import { selectAppOption } from "../utils/appSelect";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  beforeAll,
  afterAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { IntentRoutingPanel } from "@/components/routing/IntentRoutingPanel";
import { intentRoutingApi, type RoutingGroup } from "@/lib/api/intentRouting";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import { nativeSubscriptionApi } from "@/lib/api/nativeSubscription";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { routingTemplates } from "@/lib/routingGroupTransfer";
vi.mock("sonner", () => ({ toast: { success: vi.fn(), message: vi.fn() } }));

vi.mock("@/components/routing/RoutingClient", () => ({
  RoutingClient: () => null,
}));
vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(async () => []),
    codexQuota: vi.fn(),
  },
}));
vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: { quota: vi.fn() },
}));
vi.mock("@/lib/api/intentRouting", () => ({
  intentRoutingApi: {
    list: vi.fn(),
    providers: vi.fn(),
    save: vi.fn(),
    remove: vi.fn(),
    preview: vi.fn(),
    trial: vi.fn(),
    setAutoCreate: vi.fn(),
    setAutoGroupSkip: vi.fn(),
    setDiscoveredEnabled: vi.fn(),
    setContextSettings: vi.fn(),
    refreshCatalogs: vi.fn(),
    discoverCatalogs: vi.fn(),
    autoGroupSkip: vi.fn(),
    matchedMembers: vi.fn(),
    arrange: vi.fn(),
    restore: vi.fn(),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    i18n: { language: "en" },
    t: (key: string, values?: { budgetMs?: number }) =>
      values?.budgetMs === undefined ||
      !key.startsWith("intentRouting.trialOutcome.timeout")
        ? key
        : `${key} ${values.budgetMs}`,
  }),
}));

const group = {
  id: "fixture",
  name: "Fixture group",
  revision: 1,
  client: "codex" as const,
  enabled: false as const,
  intentRecognitionEnabled: false,
  classifierTarget: null,
  classificationBudgetMs: 2000,
  members: [{ id: "member", providerId: "provider", modelId: "fixture-model" }],
  defaultMemberId: "member",
  fallbackMemberIds: [],
  rules: [
    {
      id: "debug",
      name: "Debug",
      description: "Diagnose bugs",
      enabled: true,
      targetMemberId: "member",
    },
  ],
};

function mount(
  client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  }),
  maskAccounts = false,
  onAddProvider?: () => void,
) {
  return render(
    <QueryClientProvider client={client}>
      <div role="tabpanel" tabIndex={0}>
        <IntentRoutingPanel
          maskAccounts={maskAccounts}
          onAddProvider={onAddProvider}
        />
      </div>
    </QueryClientProvider>,
  );
}

const keepRankedOrder = () => 0.999999;

function drawTemplateSuggestions(
  trigger: HTMLElement,
  random = keepRankedOrder,
) {
  const draw = vi.spyOn(Math, "random").mockImplementation(random);
  try {
    fireEvent.click(trigger);
  } finally {
    draw.mockRestore();
  }
}

async function openManagement() {
  const manage = await screen.findByRole("button", {
    name: "routingPage.manage",
  });
  await waitFor(() => expect(manage).toBeEnabled());
  fireEvent.click(manage);
}

const chooseCustom = () =>
  fireEvent.click(
    screen.getByRole("button", { name: "routingPage.templateCustom" }),
  );

function mockTwoTaskModels() {
  vi.mocked(intentRoutingApi.providers).mockResolvedValue([
    {
      id: "provider",
      name: "Fixture provider",
      taskEligible: true,
      classifierEligible: false,
      models: [
        { id: "fixture-model", name: "Fixture model" },
        { id: "another-model", name: "Another model" },
      ],
    },
  ]);
}

function expectModelPickerWheelAllowed() {
  const list = screen.getByRole("radiogroup", { name: "modelPicker.models" });
  // JSDOM has no layout. Give the scroll lock a list with room to scroll.
  list.style.overflowY = "auto";
  Object.defineProperties(list, {
    clientHeight: { value: 200 },
    scrollHeight: { value: 800 },
  });
  const wheel = { deltaY: 60, cancelable: true };
  const row = within(list).queryAllByRole("radio").at(-1);
  expect(fireEvent.wheel(row ?? list, wheel)).toBe(true);
  expect(fireEvent.wheel(document.body, wheel)).toBe(false);
  list.scrollTop = 240;
  fireEvent.scroll(list);
  expect(list).toBeVisible();
}

describe("IntentRoutingPanel", () => {
  it("unlocks an acknowledged group toggle while catalog queries are still pending", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const view = mount(client);
    await screen.findByText(group.name);
    const row = view.container.querySelector(
      '[data-routing-group="fixture"]',
    ) as HTMLElement;
    const toggle = within(row).getByRole("switch");
    vi.spyOn(client, "invalidateQueries").mockReturnValue(
      new Promise(() => {}),
    );
    fireEvent.click(toggle);
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).toBeChecked();
  });
  it.each([false, true])(
    "keeps independent rows usable and does not release another row's operation (first fails: %s)",
    async (firstFails) => {
      const second = { ...group, id: "second-group", name: "Second group" };
      const saved = new Map<string, RoutingGroup>([
        [group.id, structuredClone(group)],
        [second.id, second],
      ]);
      const finishes = new Map<
        string,
        { succeed: () => void; fail: () => void }
      >();
      vi.mocked(intentRoutingApi.list).mockImplementation(async () => ({
        groups: [...saved.values()],
        autoCreate: false,
        omittedGroups: 0,
      }));
      vi.mocked(intentRoutingApi.save).mockImplementation(
        (value) =>
          new Promise((resolve, reject) => {
            finishes.set(value.id, {
              succeed: () => {
                const next = { ...value, revision: value.revision + 1 };
                saved.set(value.id, next);
                resolve(next);
              },
              fail: () => reject("intentRouting.conflict"),
            });
          }),
      );
      const { container } = mount();
      await screen.findByText("Second group");
      const row = (id: string) =>
        container.querySelector<HTMLElement>(`[data-routing-group="${id}"]`)!;
      const toggle = (id: string) => within(row(id)).getByRole("switch");
      fireEvent.click(toggle(group.id));
      expect(row(group.id)).toHaveAttribute("aria-busy", "true");
      expect(toggle(group.id)).toBeDisabled();
      expect(toggle(second.id)).toBeEnabled();
      expect(
        within(row(second.id)).getByRole("button", { name: "common.edit" }),
      ).toBeEnabled();
      for (const name of [
        "routingPage.refresh",
        "routingPage.manage",
        "intentRouting.create",
      ])
        expect(screen.getByRole("button", { name })).toBeEnabled();
      fireEvent.click(toggle(group.id));
      expect(intentRoutingApi.save).toHaveBeenCalledTimes(1);
      fireEvent.click(toggle(second.id));
      expect(intentRoutingApi.save).toHaveBeenCalledTimes(2);
      await act(async () => {
        if (firstFails) finishes.get(group.id)!.fail();
        else finishes.get(group.id)!.succeed();
      });
      await waitFor(() =>
        expect(row(group.id)).toHaveAttribute("aria-busy", "false"),
      );
      expect(toggle(group.id)).toBeEnabled();
      expect(row(second.id)).toHaveAttribute("aria-busy", "true");
      expect(toggle(second.id)).toBeDisabled();
      await act(async () => finishes.get(second.id)!.succeed());
      await waitFor(() => expect(toggle(second.id)).toBeEnabled());
      expect(row(second.id)).toHaveAttribute("aria-busy", "false");
      expect(toggle(second.id)).toHaveAttribute("aria-checked", "true");
    },
  );

  it("keeps other rows available while deletion refreshes the list", async () => {
    const second = { ...group, id: "second-group", name: "Second group" };
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, second],
      autoCreate: false,
      omittedGroups: 0,
    });
    let finishRefresh!: () => void;
    vi.mocked(intentRoutingApi.remove).mockImplementation(async () => {
      vi.mocked(intentRoutingApi.list).mockReturnValue(
        new Promise((resolve) => {
          finishRefresh = () =>
            resolve({ groups: [second], autoCreate: false, omittedGroups: 0 });
        }),
      );
    });
    const { container } = mount();
    await screen.findByText("Second group");
    const row = container.querySelector<HTMLElement>(
      '[data-routing-group="fixture"]',
    )!;
    fireEvent.click(within(row).getByRole("button", { name: "common.delete" }));
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.confirmDelete" }),
    );
    await waitFor(() => expect(finishRefresh).toBeTypeOf("function"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    const secondRow = container.querySelector<HTMLElement>(
      '[data-routing-group="second-group"]',
    )!;
    expect(within(secondRow).getByRole("switch")).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "routingPage.refresh" }),
    ).toBeEnabled();
    await act(async () => finishRefresh());
  });

  it("keeps template creation separate from batch selection", async () => {
    mount();
    await screen.findByText("Fixture group");
    const table = screen.getByRole("table", { name: "intentRouting.title" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent),
    ).toEqual([
      "routingPage.groupName",
      "routingPage.modelCount",
      "routingPage.requestCount",
      "routingPage.actions",
    ]);
    expect(
      screen.queryByRole("switch", { name: "intentRouting.autoCreate" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "routingPage.manage" }));
    expect(
      screen.queryByRole("switch", { name: "intentRouting.autoCreate" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("routingPage.templateSmart"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "Fixture group" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "routingPage.removeSelected" }),
    ).toBeDisabled();
    const row = within(table).getAllByRole("row")[1];
    expect(
      within(row).getAllByTitle("routingPage.groupStatsUnavailable"),
    ).toHaveLength(1);
  });

  it("separates publication from row actions and keeps ordering at the trailing edge", async () => {
    mount();
    const heading = await screen.findByRole("heading", {
      name: "Fixture group",
    });
    const row = heading.closest("tr")!;
    const cells = within(row).getAllByRole("cell");
    expect(cells).toHaveLength(4);
    expect(
      within(row).queryByText("vary-auto/fixture"),
    ).not.toBeInTheDocument();
    expect(within(cells[0]).queryByRole("combobox")).not.toBeInTheDocument();
    expect(within(cells[3]).queryByRole("switch")).not.toBeInTheDocument();
    expect(
      within(cells[3]).queryByRole("combobox", {
        name: "intentRouting.routingStrategy",
      }),
    ).not.toBeInTheDocument();
    const publication = within(cells[0]).getByRole("switch", {
      name: "intentRouting.publishGateway Fixture group",
    });
    expect(publication).not.toBeChecked();
    expect(publication).toHaveAttribute("data-state", "unchecked");
    expect(publication).toHaveTextContent(/^$/);
    const edit = within(cells[3]).getByRole("button", { name: "common.edit" });
    expect(edit).toHaveTextContent(/^$/);
    expect(edit).toHaveAttribute("title", "common.edit");
    const reorder = within(cells[3]).getByRole("button", {
      name: "routingPage.arrangeGroup Fixture group",
    });
    expect(
      within(cells[0]).queryByRole("button", {
        name: "routingPage.arrangeGroup Fixture group",
      }),
    ).not.toBeInTheDocument();
    expect(reorder.parentElement?.lastElementChild).toBe(reorder);
    expect(within(reorder).queryByRole("img")).not.toBeInTheDocument();
    fireEvent.click(publication);
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "fixture",
          enabled: true,
          members: group.members,
        }),
      ),
    );
  });

  it.each([
    ["absent", null, 3],
    [
      "a separate classifier",
      { providerId: "provider", modelId: "classifier" },
      4,
    ],
    [
      "an explicit member",
      { providerId: "provider", modelId: "fixture-model" },
      3,
    ],
    [
      "a pattern match",
      { providerId: "provider", modelId: "matched-model" },
      3,
    ],
    [
      "the same model at a different supplier",
      { providerId: "other", modelId: "fixture-model" },
      4,
    ],
  ] as const)(
    "counts configured models with %s",
    async (_, classifierTarget, expected) => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [
          {
            ...group,
            classifierTarget,
            intentRecognitionEnabled: classifierTarget !== null,
            members: [
              ...group.members,
              { ...group.members[0], id: "fixed-effort", effort: "high" },
            ],
            match: ["matched-*"],
          },
        ],
        matchedMembers: {
          [group.id]: [
            { id: "matched", providerId: "provider", modelId: "matched-model" },
          ],
        },
        autoCreate: false,
        omittedGroups: 0,
      });
      mount();
      const row = (
        await screen.findByRole("heading", { name: group.name })
      ).closest("tr")!;
      expect(within(row).getAllByRole("cell")[1]).toHaveTextContent(
        new RegExp(`^${expected}$`),
      );
    },
  );

  it("keeps the member summary readable and opens the editor from it without a native tooltip", async () => {
    mount();
    const table = await screen.findByRole("table", {
      name: "intentRouting.title",
    });
    const summary = await within(table).findByText(/ · fixture-model$/);
    expect(summary.closest("[title]")).toBeNull();
    fireEvent.click(summary);
    expect(await screen.findByRole("dialog")).toBeVisible();
  });

  it("omits import, More and removed-group controls even when archived groups exist", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group],
      autoCreate: false,
      omittedGroups: 0,
      removedGroups: [{ id: "archived", name: "Archived group" }],
    });
    mount();
    await screen.findByText("Fixture group");
    expect(
      screen.queryByRole("button", { name: "routingPage.import" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /routingPage.more/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "intentRouting.removedGroups" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem")).not.toBeInTheDocument();
    expect(intentRoutingApi.restore).not.toHaveBeenCalled();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("combines status and text filters without saving or showing a group count", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        group,
        { ...group, id: "enabled", name: "Enabled group", enabled: true },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    await screen.findByText("Fixture group");
    const status = screen.getByRole("combobox", {
      name: "routingPage.groupStatus",
    });
    selectAppOption(status, "enabled");
    expect(screen.queryByText("Fixture group")).not.toBeInTheDocument();
    expect(screen.getByText("Enabled group")).toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("searchbox", { name: "intentRouting.filter" }),
      { target: { value: "missing" } },
    );
    expect(screen.getByText("intentRouting.noMatches")).toBeInTheDocument();
    expect(screen.queryByTestId("routing-group-count")).not.toBeInTheDocument();
    selectAppOption(status, "disabled");
    fireEvent.change(
      screen.getByRole("searchbox", { name: "intentRouting.filter" }),
      { target: { value: "fixture-model" } },
    );
    expect(screen.getByText("Fixture group")).toBeInTheDocument();
    expect(screen.queryByText("Enabled group")).not.toBeInTheDocument();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("counts configured and matched model members including models on the same upstream", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          members: [
            ...group.members,
            { id: "second-model", providerId: "provider", modelId: "second" },
          ],
        },
      ],
      matchedMembers: {
        fixture: [
          {
            id: "matched",
            providerId: "other-provider",
            modelId: "matched-model",
          },
          {
            id: "same-provider",
            providerId: "provider",
            modelId: "third-model",
          },
        ],
      },
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    await screen.findByText("Fixture group");
    const row = screen.getByText("Fixture group").closest("tr")!;
    expect(within(row).getAllByRole("cell")[1]).toHaveTextContent(/^4$/);
  });

  it("puts models under the name without a strategy selector and keeps rate-limit help", async () => {
    mockTwoTaskModels();
    mount();
    await screen.findByText("Fixture group");
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.create" }),
    );
    chooseCustom();
    const form = screen.getByRole("form");
    expect(
      within(form).queryByRole("radiogroup", {
        name: "intentRouting.routingStrategy",
      }),
    ).not.toBeInTheDocument();
    const fields = [...form.querySelectorAll("[data-routing-field]")].map(
      (node) => node.getAttribute("data-routing-field"),
    );
    expect(fields.slice(0, 4)).toEqual([
      "name",
      "members",
      "patterns",
      "model-lists",
    ]);
    expect(screen.getByLabelText("intentRouting.name")).toHaveAttribute(
      "placeholder",
      "intentRouting.namePlaceholder",
    );
    const advanced = within(form)
      .getByText("intentRouting.advancedSettings")
      .closest("details");
    expect(advanced).not.toHaveAttribute("open");
    expect(
      within(form).getByRole("radiogroup", { name: "intentRouting.sink" }),
    ).not.toBeVisible();
    expect(advanced).toContainElement(
      form.querySelector('[data-routing-field="patterns"]'),
    );
    expect(advanced).toContainElement(
      form.querySelector('[data-routing-field="model-lists"]'),
    );
    fireEvent.click(within(form).getByText("intentRouting.advancedSettings"));
    expect(
      within(form).getByRole("radiogroup", { name: "intentRouting.sink" }),
    ).toBeVisible();
    expect(
      within(form).getByText("intentRouting.sinkKeepHelp"),
    ).toBeInTheDocument();
    fireEvent.click(
      within(form).getByRole("radio", { name: "intentRouting.sinkBack" }),
    );
    expect(
      within(form).getByText("intentRouting.sinkHelp"),
    ).toBeInTheDocument();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("retains advanced values when the section is collapsed and the group is saved", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          firstToken: 30,
          patience: 120,
          contextWindow: 272000,
          affinity: "strict",
          unlists: true,
          levels: ["low", "high"],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    const advanced = screen.getByText("intentRouting.advancedSettings");
    expect(advanced.closest("details")).not.toHaveAttribute("open");
    fireEvent.click(advanced);
    fireEvent.click(screen.getByRole("radio", { name: "1 min" }));
    fireEvent.change(screen.getByLabelText("intentRouting.contextTokens"), {
      target: { value: "200k" },
    });
    fireEvent.click(advanced);
    expect(
      screen.getByLabelText("intentRouting.contextTokens"),
    ).not.toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          firstToken: 60,
          patience: 120,
          contextWindow: 200000,
          affinity: "strict",
          unlists: true,
          levels: ["low", "high"],
        }),
      ),
    );
  });

  it("preserves sink on edit for ordered and fastest groups", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [{ ...group, routing: "fastest", sink: true }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({ routing: "fastest", sink: true }),
      ),
    );
  });

  it("passes the provider's saved image through to the group row", async () => {
    const icon = "data:image/png;base64,c3ludGhldGlj";
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Synthetic provider",
        icon,
        taskEligible: true,
        classifierEligible: false,
      },
    ]);
    mount();
    const heading = await screen.findByRole("heading", {
      name: "Fixture group",
    });
    const row = heading.closest("tr")!;
    expect(row.querySelector(".mp-icon-disc img")).toHaveAttribute("src", icon);
    expect(row.querySelector(".mp-icon-stack")).not.toHaveTextContent("SP");
  });

  it("masks configured supplier names and account addresses while preserving configured icons", async () => {
    const options = [
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
      },
      {
        id: "official",
        name: "Codex · fixture.account@example.test",
        icon: "openai",
        appType: "codex",
        taskEligible: true,
        classifierEligible: false,
      },
    ];
    vi.mocked(intentRoutingApi.providers).mockResolvedValue(options);
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          members: [
            ...group.members,
            { id: "official", providerId: "official", modelId: "model-b" },
          ],
        },
      ],
      autoCreate: true,
      omittedGroups: 0,
    });
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const view = mount(client, true);
    await screen.findByText("Fixture group");
    const summary = document.querySelector(".rt-group-summary");
    expect(summary).toHaveTextContent("· fixture-model");
    expect(summary).not.toHaveTextContent("Fixture provider");
    expect(summary).toHaveTextContent("Codex ·");
    expect(summary).not.toHaveTextContent("fixture.account@example.test");
    expect(summary?.querySelector(".account-mask")).not.toBeNull();
    expect(summary?.querySelector("[title]")).toBeNull();
    expect(document.querySelector(".mp-icon-stack")).not.toHaveTextContent(
      "FP",
    );
    expect(document.querySelectorAll(".mp-icon-disc svg")).toHaveLength(2);
    expect(view.container.innerHTML).not.toContain(
      "fixture.account@example.test",
    );
    expect(view.container.innerHTML).not.toContain("routingPage.maskedAccount");

    await userEvent.click(screen.getByRole("button", { name: "common.edit" }));
    expect(document.body.innerHTML).not.toContain(
      "fixture.account@example.test",
    );
    expect(
      screen.getByRole("dialog").querySelectorAll(".rt-provider-models svg"),
    ).not.toHaveLength(0);
    await userEvent.click(
      screen.getByRole("button", { name: "common.cancel" }),
    );

    view.rerender(
      <QueryClientProvider client={client}>
        <IntentRoutingPanel maskAccounts={false} />
      </QueryClientProvider>,
    );
    expect(document.querySelector(".rt-group-summary")).toHaveTextContent(
      "fixture.account@example.test",
    );
    expect(document.querySelector(".rt-group-summary")).toHaveTextContent(
      "Fixture provider · fixture-model",
    );
    expect(client.getQueryData(["intent-routing-providers"])).toEqual(options);
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("loads missing account catalogs on entry and reloads groups after discovery", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: true,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.discoverCatalogs).mockImplementation(
      async () => {
        vi.mocked(intentRoutingApi.list).mockResolvedValue({
          groups: [{ ...group, autoGenerated: true }],
          autoCreate: true,
          omittedGroups: 0,
        });
        return [{ providerId: "provider", status: "updated", modelCount: 2 }];
      },
    );
    mount();
    expect(await screen.findByText("Fixture group")).toBeInTheDocument();
    expect(intentRoutingApi.discoverCatalogs).toHaveBeenCalledOnce();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it.each([true, false])(
    "hides discovery controls without changing the saved preference %s",
    async (autoCreate) => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [group],
        autoCreate,
        omittedGroups: 0,
      });
      mount();
      await openManagement();
      expect(
        screen.queryByText("intentRouting.autoCreate"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("switch", { name: "intentRouting.autoCreate" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "intentRouting.skipProviders" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByText("routingPage.templateSmart"),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("checkbox", { name: "Fixture group" }),
      ).toBeInTheDocument();
      expect(intentRoutingApi.setAutoCreate).not.toHaveBeenCalled();
      expect(intentRoutingApi.setAutoGroupSkip).not.toHaveBeenCalled();
    },
  );

  it("toggles publication of an automatic group without deleting or editing it", async () => {
    const automatic = {
      ...group,
      id: "auto-fixture",
      revision: 0,
      enabled: true,
      autoGenerated: true,
    };
    const catalog = { groups: [automatic], autoCreate: true, omittedGroups: 0 };
    vi.mocked(intentRoutingApi.list).mockResolvedValue(catalog);
    vi.mocked(intentRoutingApi.setDiscoveredEnabled).mockImplementation(
      async (_, enabled) => {
        const saved = { ...automatic, enabled };
        vi.mocked(intentRoutingApi.list).mockResolvedValue({
          ...catalog,
          groups: [saved],
        });
        return saved;
      },
    );
    mount();
    const toggle = await screen.findByRole("switch", {
      name: "intentRouting.publishGateway Fixture group",
    });
    await userEvent.click(toggle);
    await waitFor(() => expect(toggle).not.toBeChecked());
    await userEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(intentRoutingApi.setDiscoveredEnabled).toHaveBeenNthCalledWith(
      1,
      automatic.id,
      false,
    );
    expect(intentRoutingApi.setDiscoveredEnabled).toHaveBeenNthCalledWith(
      2,
      automatic.id,
      true,
    );
    expect(intentRoutingApi.remove).not.toHaveBeenCalled();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });
  it("omits the duplicate advanced member editor and preserves saved reasoning", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        { ...group, members: [{ ...group.members[0], effort: "high" }] },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    expect(
      screen.getByRole("checkbox", { name: "fixture-model" }),
    ).toBeChecked();
    expect(document.querySelector(".rt-advanced-members")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          members: [expect.objectContaining({ id: "member", effort: "high" })],
        }),
      ),
    );
  });
  it("removes a member as the reference implementation does: promotes remaining rule targets and drops targetless rules", async () => {
    const members = [
      group.members[0],
      { id: "next", providerId: "provider", modelId: "next-model" },
      { id: "last", providerId: "provider", modelId: "last-model" },
    ];
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          members,
          off: ["member"],
          fallbackMemberIds: ["next", "last"],
          rules: [
            { ...group.rules[0], fallbackMemberIds: ["next", "last"] },
            { ...group.rules[0], id: "only", fallbackMemberIds: [] },
            {
              ...group.rules[0],
              id: "later",
              targetMemberId: "last",
              fallbackMemberIds: ["member", "next"],
            },
          ],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    await user.click(screen.getByRole("checkbox", { name: "fixture-model" }));
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalled());
    const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    expect(saved.members.map((member) => member.id)).toEqual(["next", "last"]);
    expect(saved.defaultMemberId).toBe("next");
    expect(saved.off).toEqual([]);
    expect(saved.rules).toEqual([
      expect.objectContaining({
        id: "debug",
        targetMemberId: "next",
        fallbackMemberIds: ["last"],
      }),
      expect.objectContaining({
        id: "later",
        targetMemberId: "last",
        fallbackMemberIds: ["next"],
      }),
    ]);
  });
  it("selects an intent classifier from the shared model picker without a separate provider/model form", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "classifier",
        name: "Synthetic classifier",
        taskEligible: true,
        classifierEligible: true,
        models: [{ id: "tiny", name: "Tiny classifier" }],
      },
    ]);
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    expect(
      screen
        .getByLabelText("intentRouting.classifierProvider")
        .closest("details"),
    ).not.toHaveAttribute("open");
    await user.click(
      screen.getByRole("button", { name: "routingPage.intentToldBy" }),
    );
    await user.click(screen.getByRole("radio", { name: /Tiny classifier/ }));
    expect(
      screen.getByRole("button", { name: "routingPage.intentToldBy" }),
    ).toHaveTextContent("Tiny classifier");
    await user.click(
      screen.getByRole("button", { name: "routingPage.intentToldBy" }),
    );
    await user.click(
      screen.getByRole("button", { name: "intentRouting.none" }),
    );
    const classifier = screen.getByRole("button", {
      name: "routingPage.intentToldBy",
    });
    expect(classifier).toHaveTextContent("routingPage.chooseClassifier");
    await user.click(classifier);
    await user.click(screen.getByRole("radio", { name: /Tiny classifier/ }));
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          intentRecognitionEnabled: true,
          classifierTarget: { providerId: "classifier", modelId: "tiny" },
        }),
      ),
    );
  });
  it("shows missing template requirements without disabling the chooser", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const create = screen.getByRole("button", { name: "intentRouting.create" });
    await waitFor(() => expect(create).toBeEnabled());
    fireEvent.click(create);
    const chooser = screen.getByRole("dialog", {
      name: "routingPage.templates",
    });
    expect(
      within(chooser).getAllByText("routingPage.templateNeed_provider"),
    ).toHaveLength(4);
    const actions = within(chooser).getAllByRole("button", {
      name: /^routingPage.template(Smart|Capability|Speed|Expiry|Custom)$/,
    });
    expect(actions).toHaveLength(5);
    for (const action of actions) expect(action).toBeEnabled();
    expect(
      screen.queryByText("routingPage.templateFallback"),
    ).not.toBeInTheDocument();
  });
  it.each([0, 1])(
    "warns on choosing a template or Custom with %i available models",
    async (count) => {
      vi.mocked(intentRoutingApi.providers).mockResolvedValue([
        {
          id: "task",
          name: "Synthetic task provider",
          taskEligible: true,
          classifierEligible: false,
          models: count ? [{ id: "only", name: "Only task model" }] : [],
        },
        {
          id: "classifier",
          name: "Synthetic classifier provider",
          taskEligible: false,
          classifierEligible: true,
          models: [{ id: "classify", name: "Classifier only" }],
        },
      ]);
      const addProvider = vi.fn();
      mount(undefined, false, addProvider);
      const user = userEvent.setup();
      const create = screen.getByRole("button", {
        name: "intentRouting.create",
      });
      await waitFor(() => expect(create).toBeEnabled());
      await user.click(create);
      const title = "routingPage.cannotCreateGroup";
      expect(
        screen.getByRole("dialog", { name: "routingPage.templates" }),
      ).toBeVisible();
      expect(screen.queryByRole("dialog", { name: title })).toBeNull();
      const template = screen.getByRole("button", {
        name: "routingPage.templateSmart",
      });
      await user.click(template);
      const warning = screen.getByRole("dialog", { name: title });
      expect(warning).toHaveAccessibleDescription(
        count
          ? "routingPage.oneAvailableModel"
          : "routingPage.noAvailableModels",
      );
      expect(screen.queryByRole("form")).toBeNull();
      await user.click(
        within(warning).getByRole("button", { name: "common.cancel" }),
      );
      await waitFor(() => expect(template).toHaveFocus());
      expect(addProvider).not.toHaveBeenCalled();
      await user.click(
        screen.getByRole("button", { name: "routingPage.templateCustom" }),
      );
      await user.click(
        within(screen.getByRole("dialog", { name: title })).getByRole(
          "button",
          { name: "routingPage.addProvider" },
        ),
      );
      expect(addProvider).toHaveBeenCalledOnce();
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
      expect(screen.queryByRole("form")).toBeNull();
    },
  );
  it.each([
    "templateSmart",
    "templateCapability",
    "templateSpeed",
    "templateExpiry",
  ] as const)(
    "opens %s without advanced settings and saves the reviewed draft",
    async (key) => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [],
        autoCreate: false,
        omittedGroups: 0,
      });
      const providers = [
        {
          id: "a",
          name: "Synthetic A",
          taskEligible: true,
          classifierEligible: true,
          models: [
            {
              id: "large",
              name: "Large",
              outputPrice: 15,
              maker: true,
              images: true,
              reasoning: true,
            },
            {
              id: "small-mini",
              name: "Mini",
              outputPrice: 2,
              maker: true,
              reasoning: false,
            },
          ],
        },
        {
          id: "b",
          name: "Synthetic B",
          taskEligible: true,
          classifierEligible: true,
          models: [
            {
              id: "large",
              name: "Large copy",
              outputPrice: 15,
              maker: true,
              images: true,
            },
            {
              id: "other-mini",
              name: "Other",
              outputPrice: 1,
              maker: true,
              reasoning: false,
            },
          ],
        },
      ];
      const expected = routingTemplates(providers, keepRankedOrder).find(
        (template) => template.key === key,
      )!.group;
      vi.mocked(intentRoutingApi.providers).mockResolvedValue(providers);
      mount();
      const user = userEvent.setup();
      const open = async () => {
        const create = screen.getByRole("button", {
          name: "intentRouting.create",
        });
        await waitFor(() => expect(create).toBeEnabled());
        drawTemplateSuggestions(create);
        const chooser = screen.getByRole("dialog", {
          name: "routingPage.templates",
        });
        expect(
          within(chooser).getByRole("button", {
            name: "routingPage.templateCustom",
          }),
        ).toBeEnabled();
        await user.click(
          within(chooser).getByRole("button", { name: `routingPage.${key}` }),
        );
        expect(
          screen.queryByRole("dialog", { name: "intentRouting.discardTitle" }),
        ).not.toBeInTheDocument();
        const dialog = screen.getByRole("dialog", {
          name: `routingPage.${key}`,
        });
        expect(dialog).toContainElement(screen.getByRole("form"));
        expect(
          within(dialog).queryByText("routingPage.modelOrder"),
        ).not.toBeInTheDocument();
        if (key === "templateSmart") {
          expect(
            within(dialog).queryByText("intentRouting.advancedSettings"),
          ).not.toBeInTheDocument();
          const classifier = within(dialog).getByRole("button", {
            name: "routingPage.intentToldBy",
          });
          expect(classifier).toBeVisible();
          expect(classifier.closest("details")).toBeNull();
          expect(
            within(dialog).queryByText("intentRouting.members"),
          ).not.toBeInTheDocument();
          expect(
            within(dialog).getByText("routingPage.templateSmartRules"),
          ).toBeInTheDocument();
          expect(
            dialog.querySelectorAll(".rt-template-rule-model"),
          ).toHaveLength(3);
          expect(
            within(dialog).queryByText("intentRouting.contextPolicy"),
          ).not.toBeInTheDocument();
        } else {
          expect(
            within(dialog).queryByText("intentRouting.advancedSettings"),
          ).not.toBeInTheDocument();
        }
        expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
          `routingPage.${key}`,
        );
        const ruleHint = dialog.querySelector(".rt-template-hint");
        expect(ruleHint).not.toBeNull();
        expect(
          within(ruleHint as HTMLElement).getByText(
            "routingPage.templateHintLabel",
          ),
        ).toBeVisible();
        expect(
          within(ruleHint as HTMLElement).getByText(
            `routingPage.${key}RuleHint`,
          ),
        ).toBeVisible();
        expect(dialog.querySelectorAll(".rt-template-member")).toHaveLength(
          key === "templateSmart" ? 0 : Math.min(expected.members.length, 3),
        );
        expect(
          screen.queryByRole("radiogroup", {
            name: "intentRouting.routingStrategy",
          }),
        ).not.toBeInTheDocument();
        expect(
          screen.getByRole("button", { name: "common.save" }),
        ).toBeEnabled();
        if (key !== "templateCapability" && key !== "templateExpiry") {
          expect(
            screen.queryByRole("button", {
              name: "routingPage.applyRecommendedModelOrder",
            }),
          ).not.toBeInTheDocument();
        }
        expect(intentRoutingApi.save).not.toHaveBeenCalled();
      };
      await open();
      await user.click(screen.getByRole("button", { name: "common.cancel" }));
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
      await open();
      if (key === "templateCapability") {
        const order = screen.getByRole("list", {
          name: "routingPage.modelOrder",
        });
        const [, next] = within(order).getAllByRole("listitem");
        await user.click(
          within(next).getByRole("button", { name: /^intentRouting.moveUp/ }),
        );
        expect(within(order).getAllByRole("listitem")[0]).toBe(next);
      }
      fireEvent.change(screen.getByLabelText("intentRouting.name"), {
        target: { value: "Reviewed template" },
      });
      await user.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
      const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
      expect(saved).toMatchObject({
        name: "Reviewed template",
        template: key,
        routing: expected.routing,
        classifierTarget: expected.classifierTarget,
        intentRecognitionEnabled: expected.intentRecognitionEnabled,
      });
      expect(saved.quotaOrder).toBe(expected.quotaOrder);
      expect(saved).not.toHaveProperty("policy");
      const expectedOrder =
        key === "templateCapability"
          ? [
              expected.members[1],
              expected.members[0],
              ...expected.members.slice(2),
            ]
          : expected.members;
      expect(
        saved.members.map(({ providerId, modelId }) => [providerId, modelId]),
      ).toEqual(
        expectedOrder.map(({ providerId, modelId }) => [providerId, modelId]),
      );
      expect(saved.defaultMemberId).toBe(saved.members[0].id);
      expect(saved.fallbackMemberIds).toEqual(
        saved.members.slice(1).map(({ id }) => id),
      );
      expect(
        saved.rules.map((rule) => ({
          description: rule.description,
          conditions: rule.conditions,
          target: saved.members.findIndex(
            (member) => member.id === rule.targetMemberId,
          ),
        })),
      ).toEqual(
        expected.rules.map((rule) => ({
          description: rule.description,
          conditions: rule.conditions,
          target: expected.members.findIndex(
            (member) => member.id === rule.targetMemberId,
          ),
        })),
      );
    },
  );
  it.each([
    "templateSmart",
    "templateCapability",
    "templateSpeed",
    "templateExpiry",
  ] as const)(
    "edits a saved %s group with the template editor and without advanced settings",
    async (key) => {
      const providers = [
        {
          id: "a",
          name: "Synthetic A",
          taskEligible: true,
          classifierEligible: true,
          models: [
            {
              id: "large",
              name: "Large",
              outputPrice: 15,
              maker: true,
              images: true,
              reasoning: true,
            },
            {
              id: "small-mini",
              name: "Mini",
              outputPrice: 2,
              maker: true,
              reasoning: false,
            },
          ],
        },
        {
          id: "b",
          name: "Synthetic B",
          taskEligible: true,
          classifierEligible: true,
          models: [
            {
              id: "large",
              name: "Large copy",
              outputPrice: 15,
              maker: true,
              images: true,
            },
            {
              id: "other-mini",
              name: "Other",
              outputPrice: 1,
              maker: true,
              reasoning: false,
            },
          ],
        },
      ];
      const savedGroup = {
        ...routingTemplates(providers).find((template) => template.key === key)!
          .group,
        id: `saved-${key}`,
        name: `Saved ${key}`,
        revision: 2,
      };
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [savedGroup],
        autoCreate: false,
        omittedGroups: 0,
      });
      vi.mocked(intentRoutingApi.providers).mockResolvedValue(providers);
      mount();
      const user = userEvent.setup();
      await user.click(
        await screen.findByRole("button", { name: "common.edit" }),
      );
      const dialog = screen.getByRole("dialog", { name: `Saved ${key}` });
      expect(
        within(dialog).queryByText("intentRouting.advancedSettings"),
      ).not.toBeInTheDocument();
      const ruleHint = dialog.querySelector(".rt-template-hint");
      expect(ruleHint).not.toBeNull();
      expect(
        within(ruleHint as HTMLElement).getByText(
          "routingPage.templateHintLabel",
        ),
      ).toBeVisible();
      expect(
        within(ruleHint as HTMLElement).getByText(`routingPage.${key}RuleHint`),
      ).toBeVisible();
      if (key === "templateSmart") {
        expect(
          within(dialog).getByText("routingPage.templateSmartRules"),
        ).toBeInTheDocument();
        expect(
          within(dialog).getByRole("button", {
            name: "routingPage.intentToldBy",
          }),
        ).toBeVisible();
      } else {
        expect(dialog.querySelectorAll(".rt-template-member")).toHaveLength(
          Math.min(savedGroup.members.length, 3),
        );
      }
      if (key === "templateCapability" || key === "templateExpiry") {
        expect(
          within(dialog).getByRole("button", {
            name: "routingPage.applyRecommendedModelOrder",
          }),
        ).toBeVisible();
      }
      await user.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() =>
        expect(intentRoutingApi.save).toHaveBeenCalledWith(
          expect.objectContaining({ id: `saved-${key}`, template: key }),
        ),
      );
    },
  );
  it.each(["templateSmart", "templateSpeed", "templateExpiry"] as const)(
    "also hides advanced settings when editing legacy %s groups saved without a template field",
    async (key) => {
      const expiry = Math.floor(Date.now() / 1000) + 86_400;
      const providers = [
        {
          id: "a",
          name: "Synthetic A",
          taskEligible: true,
          classifierEligible: true,
          relayExpiry: expiry,
          models: [
            { id: "large", name: "Large", outputPrice: 15, maker: true },
            { id: "mini", name: "Mini", outputPrice: 1, maker: true },
          ],
        },
        {
          id: "b",
          name: "Synthetic B",
          taskEligible: true,
          classifierEligible: true,
          models: [
            { id: "large", name: "Large copy", outputPrice: 15, maker: true },
          ],
        },
      ];
      const { template: _omitted, ...legacyGroup } = {
        ...routingTemplates(providers).find((template) => template.key === key)!
          .group,
        id: `legacy-${key}`,
        name: `Legacy ${key}`,
        revision: 1,
      };
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [legacyGroup],
        autoCreate: false,
        omittedGroups: 0,
      });
      vi.mocked(intentRoutingApi.providers).mockResolvedValue(providers);
      mount();
      const user = userEvent.setup();
      await user.click(
        await screen.findByRole("button", { name: "common.edit" }),
      );
      const dialog = screen.getByRole("dialog", { name: `Legacy ${key}` });
      expect(
        within(dialog).queryByText("intentRouting.advancedSettings"),
      ).not.toBeInTheDocument();
      expect(
        within(dialog).getByText(`routingPage.${key}RuleHint`),
      ).toBeVisible();
      if (key === "templateExpiry") {
        expect(dialog.querySelector("time")).toHaveAttribute(
          "datetime",
          new Date(expiry * 1000).toISOString(),
        );
        expect(
          within(dialog).getByText("routingPage.memberExpiryUnknown"),
        ).toBeVisible();
      } else {
        expect(dialog.querySelector("time")).toBeNull();
      }
    },
  );
  it("shows actual account reset windows and WorkBuddy credit expiry in the expiry editor", async () => {
    const date = (hours: number) =>
      new Date(Date.now() + hours * 3_600_000).toISOString();
    const fiveHour = date(4);
    const sevenDay = date(36);
    const creditsReset = date(24);
    const creditsExpiry = date(72);
    const quota = {
      tool: "fixture",
      credentialStatus: "valid" as const,
      credentialMessage: null,
      success: true,
      tiers: [],
      extraUsage: null,
      error: null,
      queriedAt: null,
    };
    const providers = ["codex", "gemini", "workbuddy"].map((id) => ({
      id,
      name: id,
      taskEligible: true,
      classifierEligible: false,
      models: [{ id: `${id}-model`, name: id }],
    }));
    vi.mocked(intentRoutingApi.providers).mockResolvedValue(providers);
    vi.mocked(routingUpstreamsApi.list).mockResolvedValueOnce([
      {
        schemaVersion: 1,
        id: "codex",
        name: "codex",
        revision: 1,
        hasApiKey: false,
        source: { kind: "codex" },
        codexSettings: {
          accounts: ["fixture-codex-account"],
          mode: "vendor",
          models: [],
          groupsOnly: false,
        },
      },
      {
        schemaVersion: 1,
        id: "gemini",
        name: "gemini",
        revision: 1,
        hasApiKey: false,
        source: {
          kind: "api",
          baseUrl: "https://gemini.example",
          apiFormat: "gemini_native",
          mode: "vendor",
          models: [],
          catalog: [],
          groupsOnly: false,
        },
      },
      {
        schemaVersion: 1,
        id: "workbuddy",
        name: "workbuddy",
        revision: 1,
        hasApiKey: false,
        source: {
          kind: "subscription",
          provider: "workbuddy",
          accountId: "fixture-workbuddy-account",
          mode: "vendor",
          models: [],
          catalog: [],
          groupsOnly: false,
        },
      },
    ]);
    vi.mocked(routingUpstreamsApi.codexQuota).mockResolvedValueOnce({
      ...quota,
      tiers: [
        { name: "five_hour", utilization: 20, resetsAt: fiveHour },
        { name: "seven_day", utilization: 25, resetsAt: sevenDay },
      ],
    });
    vi.mocked(nativeSubscriptionApi.quota).mockResolvedValueOnce({
      ...quota,
      creditsExpiresAt: creditsExpiry,
      tiers: [{ name: "credits", utilization: 10, resetsAt: creditsReset }],
    });
    mount();
    const user = userEvent.setup();
    const create = screen.getByRole("button", { name: "intentRouting.create" });
    await waitFor(() => expect(create).toBeEnabled());
    drawTemplateSuggestions(create);
    await user.click(
      screen.getByRole("button", { name: "routingPage.templateExpiry" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "routingPage.templateExpiry",
    });
    const [codex, gemini, workbuddy] = within(dialog).getAllByRole("listitem");
    await waitFor(() => {
      expect(
        [...codex.querySelectorAll("time")].map((item) => item.dateTime),
      ).toEqual([fiveHour, sevenDay]);
      expect(
        [...workbuddy.querySelectorAll("time")].map((item) => item.dateTime),
      ).toEqual([creditsExpiry, creditsReset]);
      expect(
        within(gemini).getByText("routingPage.memberExpiryUnknown"),
      ).toBeVisible();
    });
    expect(routingUpstreamsApi.codexQuota).toHaveBeenCalledWith(
      "fixture-codex-account",
    );
    expect(nativeSubscriptionApi.quota).toHaveBeenCalledWith(
      "workbuddy",
      "fixture-workbuddy-account",
    );
  });
  it("saves the visible capability order when older default and fallback IDs disagree", async () => {
    const providers = ["first", "second"].map((id) => ({
      id,
      name: id,
      taskEligible: true,
      classifierEligible: false,
      models: [{ id: `${id}-model`, name: id }],
    }));
    const template = routingTemplates(providers).find(
      ({ key }) => key === "templateCapability",
    )!.group;
    const [first, second] = template.members;
    vi.mocked(intentRoutingApi.providers).mockResolvedValue(providers);
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...template,
          id: "saved-capability",
          name: "Saved capability",
          revision: 1,
          members: [second, first],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getAllByRole("listitem")[0]).toHaveTextContent(
      second.providerId,
    );
    await user.click(
      within(dialog).getByRole("button", { name: "common.save" }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          members: [second, first],
          defaultMemberId: second.id,
          fallbackMemberIds: [first.id],
        }),
      ),
    );
  });
  it("saves the expiry template in the visible order after the user overrides the recommendation", async () => {
    const expiry = Math.floor(Date.now() / 1000) + 86_400;
    const providers = ["first", "middle", "last"].map((id) => ({
      id,
      name: id,
      taskEligible: true,
      classifierEligible: false,
      ...(id === "first" ? { relayExpiry: expiry } : {}),
      models: [{ id: `${id}-model`, name: id }],
    }));
    vi.mocked(intentRoutingApi.providers).mockResolvedValue(providers);
    mount();
    const user = userEvent.setup();
    const create = screen.getByRole("button", { name: "intentRouting.create" });
    await waitFor(() => expect(create).toBeEnabled());
    drawTemplateSuggestions(create);
    await user.click(
      screen.getByRole("button", { name: "routingPage.templateExpiry" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "routingPage.templateExpiry",
    });
    const rows = () => within(dialog).getAllByRole("listitem");
    expect(rows()[0].querySelector("time")).toHaveAttribute(
      "datetime",
      new Date(expiry * 1000).toISOString(),
    );
    expect(
      within(rows()[1]).getByText("routingPage.memberExpiryUnknown"),
    ).toBeVisible();
    await user.click(
      within(rows()[1]).getByRole("button", {
        name: "intentRouting.moveUp middle",
      }),
    );
    expect(rows()[0]).toHaveTextContent("middle");
    await user.click(
      within(dialog).getByRole("button", {
        name: "routingPage.applyRecommendedModelOrder",
      }),
    );
    expect(rows()[0]).toHaveTextContent("first");
    await user.click(
      within(rows()[1]).getByRole("button", {
        name: "intentRouting.moveUp middle",
      }),
    );
    expect(rows()[0]).toHaveTextContent("middle");
    await user.click(
      within(dialog).getByRole("button", { name: "common.save" }),
    );
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
    const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    expect(saved.members.map(({ providerId }) => providerId)).toEqual([
      "middle",
      "first",
      "last",
    ]);
    expect(saved.defaultMemberId).toBe(saved.members[0].id);
    expect(saved.fallbackMemberIds).toEqual([
      saved.members[1].id,
      saved.members[2].id,
    ]);
    expect(saved.quotaOrder).toBe("earliest_expiry");
  });
  it("chooses smart rule targets independently from the full task catalog", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "task",
        name: "Synthetic task provider",
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: "strong", name: "Strong", maker: true, outputPrice: 12 },
          { id: "cheap", name: "Cheap", maker: true, outputPrice: 1 },
          { id: "extra", name: "Extra", outputPrice: 12 },
          { id: "alternate", name: "Alternate", outputPrice: 12 },
        ],
      },
      {
        id: "classifier",
        name: "Synthetic classifier provider",
        taskEligible: false,
        classifierEligible: true,
        models: [{ id: "classify", name: "Classify" }],
      },
    ]);
    mount();
    const user = userEvent.setup();
    const create = screen.getByRole("button", { name: "intentRouting.create" });
    await waitFor(() => expect(create).toBeEnabled());
    await user.click(create);
    await user.click(
      screen.getByRole("button", { name: "routingPage.templateSmart" }),
    );
    const target = (rule: string) =>
      screen.getByRole("button", {
        name: `intentRouting.target routingPage.templateSmart${rule}Rule`,
      });
    const pick = async (rule: string, model: string) => {
      await user.click(target(rule));
      expect(screen.queryByRole("radio", { name: /Classify/ })).toBeNull();
      await user.click(screen.getByRole("radio", { name: new RegExp(model) }));
      expect(target(rule)).toHaveTextContent(model);
    };
    await pick("Hard", "Extra");
    await pick("Easy", "Alternate");
    await pick("Easy", "Extra");
    expect(target("Compact")).toHaveTextContent("Strong");
    await user.click(
      screen.getByRole("button", { name: "routingPage.intentToldBy" }),
    );
    await user.click(screen.getByRole("radio", { name: /Classify/ }));
    expect(
      screen.getByRole("spinbutton", { name: "intentRouting.budget" }),
    ).toHaveValue(10000);
    expect(screen.getByText("intentRouting.trialTitle")).toBeVisible();
    expect(
      screen.getByText("routingPage.templateSmartHardDescription"),
    ).toBeVisible();
    expect(
      screen.getByText("routingPage.templateSmartEasyDescription"),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
    const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    expect(saved.classificationBudgetMs).toBe(10000);
    expect(saved.classifierTarget).toEqual({
      providerId: "classifier",
      modelId: "classify",
    });
    expect(saved.members.map(({ modelId }) => modelId)).toEqual([
      "strong",
      "extra",
    ]);
    const extra = saved.members.find(({ modelId }) => modelId === "extra")!;
    expect(saved.defaultMemberId).toBe(extra.id);
    expect(saved.fallbackMemberIds).toEqual([saved.members[0].id]);
    expect(
      saved.rules.slice(1).map(({ targetMemberId }) => targetMemberId),
    ).toEqual([extra.id, extra.id]);
  });
  it("opens classifier help by click or keyboard in the smart template editor", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "task",
        name: "Synthetic provider",
        taskEligible: true,
        classifierEligible: true,
        models: [
          { id: "strong", name: "Strong", maker: true, outputPrice: 12 },
          { id: "cheap", name: "Cheap", outputPrice: 1 },
        ],
      },
    ]);
    mount();
    const user = userEvent.setup();
    const create = screen.getByRole("button", { name: "intentRouting.create" });
    await waitFor(() => expect(create).toBeEnabled());
    drawTemplateSuggestions(create);
    await user.click(
      screen.getByRole("button", { name: "routingPage.templateSmart" }),
    );

    const help = screen.getByRole("button", {
      name: "intentRouting.classifierHelp",
    });
    await user.click(help);
    expect(help).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("intentRouting.classifierNotice")).toBeVisible();
    await user.keyboard("{Escape}");
    expect(help).toHaveFocus();
    expect(screen.queryByText("intentRouting.classifierNotice")).toBeNull();

    await user.keyboard("{Enter}");
    expect(screen.getByText("intentRouting.classifierNotice")).toBeVisible();
    await user.click(
      screen.getByRole("textbox", { name: "intentRouting.name" }),
    );
    expect(screen.queryByText("intentRouting.classifierNotice")).toBeNull();
    expect(screen.getByRole("form")).toBeVisible();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });
  it("removes hidden preset models when resaving an oversized smart group", async () => {
    const members = [
      "old-primary",
      "old-backup",
      "compact",
      "hard",
      "easy",
    ].map((modelId) => ({ id: modelId, modelId, providerId: "provider" }));
    const savedGroup: RoutingGroup = {
      ...group,
      template: "templateSmart",
      members,
      defaultMemberId: "old-primary",
      fallbackMemberIds: ["old-backup", "easy"],
      off: ["old-backup"],
      rules: ["Compaction", "Hard task", "Easy task"].map((name, index) => ({
        ...group.rules[0],
        id: members[index + 2].id,
        name,
        targetMemberId: members[index + 2].id,
        fallbackMemberIds: ["old-primary", "old-backup"],
      })),
    };
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [savedGroup],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
    expect(vi.mocked(intentRoutingApi.save).mock.calls[0][0]).toMatchObject({
      members: members.slice(2),
      defaultMemberId: "hard",
      fallbackMemberIds: ["easy", "compact"],
      off: [],
      rules: savedGroup.rules.map((rule) => ({
        ...rule,
        fallbackMemberIds: [],
      })),
    });
  });
  it("can change the smart template’s default target without a price split", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "task",
        name: "Synthetic task provider",
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: "first", name: "First task model", outputPrice: 12 },
          { id: "second", name: "Second task model", outputPrice: 12 },
        ],
      },
    ]);
    mount();
    const user = userEvent.setup();
    const create = screen.getByRole("button", { name: "intentRouting.create" });
    await waitFor(() => expect(create).toBeEnabled());
    drawTemplateSuggestions(create);
    await user.click(
      screen.getByRole("button", { name: "routingPage.templateSmart" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "intentRouting.target routingPage.templateSmartHardRule",
      }),
    );
    expectModelPickerWheelAllowed();
    await user.click(screen.getByRole("radio", { name: /Second task model/ }));
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
    const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    const first = saved.members.find(({ modelId }) => modelId === "first");
    const second = saved.members.find(({ modelId }) => modelId === "second");
    expect(second?.id).toBe(saved.defaultMemberId);
    expect(
      saved.rules.map(({ name, targetMemberId }) => [name, targetMemberId]),
    ).toEqual([
      ["Compaction", first?.id],
      ["Hard task", saved.defaultMemberId],
      ["Easy task", second?.id],
    ]);
  });
  it.each([true, false])(
    "resaves and reopens a smart group without its unselected fallback (template stored: %s)",
    async (explicit) => {
      const [unused, strong, easy] = ["unused", "strong", "easy"].map(
        (modelId) => ({ id: modelId, modelId, providerId: "provider" }),
      );
      const savedGroup: RoutingGroup = {
        ...group,
        template: explicit ? "templateSmart" : undefined,
        intentRecognitionEnabled: true,
        classifierTarget: { providerId: "provider", modelId: "strong" },
        members: [unused, strong, easy],
        defaultMemberId: strong.id,
        fallbackMemberIds: [unused.id, easy.id],
        rules: [
          {
            ...group.rules[0],
            id: "compact",
            name: "Compaction",
            targetMemberId: strong.id,
            conditions: { compact: true },
          },
          {
            ...group.rules[0],
            id: "hard",
            name: "Hard task",
            targetMemberId: strong.id,
            fallbackMemberIds: [unused.id],
          },
          {
            ...group.rules[0],
            id: "easy",
            name: "Easy task",
            targetMemberId: easy.id,
          },
        ],
      };
      let storedGroup = savedGroup;
      vi.mocked(intentRoutingApi.list).mockImplementation(async () => ({
        groups: [storedGroup],
        autoCreate: false,
        omittedGroups: 0,
      }));
      vi.mocked(intentRoutingApi.save).mockImplementationOnce(async (value) => {
        storedGroup = { ...value, revision: value.revision + 1 };
        return storedGroup;
      });
      mount();
      const user = userEvent.setup();
      const open = async () =>
        user.click(await screen.findByRole("button", { name: "common.edit" }));
      await open();
      const deadline = screen.getByRole("spinbutton", {
        name: "intentRouting.budget",
      });
      expect(deadline).toHaveValue(2000);
      fireEvent.change(deadline, { target: { value: "4500" } });
      await user.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          template: "templateSmart",
          classificationBudgetMs: 4500,
          members: [strong, easy],
          defaultMemberId: strong.id,
          fallbackMemberIds: [easy.id],
          rules: expect.arrayContaining([
            expect.objectContaining({ id: "hard", fallbackMemberIds: [] }),
          ]),
        }),
      );
      await open();
      const editor = screen.getByRole("dialog", { name: savedGroup.name });
      expect(
        within(editor).getByText("routingPage.templateSmartRules"),
      ).toBeVisible();
      expect(
        within(editor).getByRole("spinbutton", {
          name: "intentRouting.budget",
        }),
      ).toHaveValue(4500);
      expect(within(editor).queryByText("intentRouting.members")).toBeNull();
      expect(within(editor).queryByText(/unused/)).toBeNull();
    },
  );
  it.each([
    [
      "templateSmart",
      [".rt-template-rule-model", ".rt-classifier-pick .rt-cond"],
    ],
    ["templateCapability", [".rt-template-member", ".rt-template-member-add"]],
    ["templateSpeed", [".rt-template-member", ".rt-template-member-add"]],
    ["templateExpiry", [".rt-template-member", ".rt-template-member-add"]],
  ] as const)(
    "allows wheel scrolling in the %s model pickers",
    async (key, triggers) => {
      vi.mocked(intentRoutingApi.providers).mockResolvedValue(
        ["a", "b"].map((id) => ({
          id,
          name: `Synthetic provider ${id}`,
          taskEligible: true,
          classifierEligible: true,
          models: Array.from({ length: 24 }, (_, index) => ({
            id: `${id}-model-${index}`,
            name: `Model ${index}`,
            outputPrice: index === 0 ? 15 : 1,
          })),
        })),
      );
      mount();
      const user = userEvent.setup();
      const create = screen.getByRole("button", {
        name: "intentRouting.create",
      });
      await waitFor(() => expect(create).toBeEnabled());
      drawTemplateSuggestions(create);
      await user.click(
        screen.getByRole("button", { name: `routingPage.${key}` }),
      );
      const dialog = screen.getByRole("dialog", { name: `routingPage.${key}` });
      for (const selector of triggers) {
        const buttons = dialog.querySelectorAll<HTMLButtonElement>(selector);
        expect(buttons.length).toBeGreaterThan(0);
        for (const trigger of buttons) {
          await user.click(trigger);
          if (key === "templateSmart") {
            const search = screen.getByRole("searchbox");
            expect(search.closest(".mp-model-picker-popover")).toHaveAttribute(
              "data-variant",
              "agent",
            );
            expect(search.closest(".mp-agent-model-picker")).not.toBeNull();
            await waitFor(() => expect(search).toHaveFocus());
          }
          expectModelPickerWheelAllowed();
          await user.keyboard("{Escape}");
          expect(
            screen.queryByRole("radiogroup", { name: "modelPicker.models" }),
          ).not.toBeInTheDocument();
          expect(trigger).toHaveFocus();
          expect(dialog).toHaveAttribute("data-state", "open");
        }
      }

      const trigger = dialog.querySelector<HTMLButtonElement>(
        triggers[triggers.length - 1],
      )!;
      await user.click(trigger);
      await user.type(screen.getByRole("searchbox"), "Model 23");
      await user.click(
        screen.getByRole("radio", { name: /Model 23.*provider b/ }),
      );
      const updatedDialog = screen.getByRole("dialog", {
        name: `routingPage.${key}`,
      });
      expect(updatedDialog).toHaveTextContent(
        "Model 23 · Synthetic provider b",
      );
      expect(
        screen.queryByRole("radiogroup", { name: "modelPicker.models" }),
      ).not.toBeInTheDocument();
    },
  );

  it.each(["templateCapability", "templateSpeed", "templateExpiry"] as const)(
    "drags a fourth model to the top of %s and saves the visible order",
    async (key) => {
      const providers = ["a", "b", "c"].map((id) => ({
        id,
        name: `Provider ${id}`,
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: `${id}-main`, name: `${id} main`, outputPrice: 20 },
          ...(id === "a"
            ? [{ id: "a-extra", name: "Extra model", outputPrice: 1 }]
            : []),
        ],
      }));
      vi.mocked(intentRoutingApi.providers).mockResolvedValue(providers);
      mount();
      const user = userEvent.setup();
      const create = screen.getByRole("button", {
        name: "intentRouting.create",
      });
      await waitFor(() => expect(create).toBeEnabled());
      drawTemplateSuggestions(create);
      await user.click(
        screen.getByRole("button", { name: `routingPage.${key}` }),
      );
      const dialog = screen.getByRole("dialog", { name: `routingPage.${key}` });
      const rows = () => within(dialog).getAllByRole("listitem");
      expect(rows()).toHaveLength(3);
      expect(within(dialog).getAllByRole("list")).toHaveLength(1);
      await user.click(
        within(dialog).getByRole("button", {
          name: "intentRouting.templateMemberAdd",
        }),
      );
      expect(screen.getByRole("radio", { name: /a main/ })).toBeDisabled();
      await user.click(screen.getByRole("radio", { name: /Extra model/ }));
      expect(rows()).toHaveLength(4);
      expect(rows()[3]).toHaveTextContent("Extra model");
      rows().forEach((row, index) =>
        vi
          .spyOn(row, "getBoundingClientRect")
          .mockReturnValue(new DOMRect(0, index * 40, 300, 40)),
      );
      const handle = within(rows()[3]).getByRole("button", {
        name: "agentConnections.dragModel Extra model",
      });
      expect(rows()[3].firstElementChild).toBe(handle);
      const pointer = (target: Node, type: string, y: number) =>
        fireEvent(
          target,
          Object.assign(
            new MouseEvent(type, {
              bubbles: true,
              cancelable: true,
              button: 0,
              clientY: y,
            }),
            { pointerId: 1, isPrimary: true, pointerType: "mouse" },
          ),
        );
      const dragToTop = () => {
        pointer(handle, "pointerdown", 140);
        pointer(document, "pointermove", 20);
      };
      const list = within(dialog).getByRole("list", {
        name: "routingPage.modelOrder",
      });
      dragToTop();
      expect(list).toHaveClass("rt-sorting");
      await user.keyboard("{Escape}");
      pointer(document, "pointerup", 20);
      expect(dialog).toHaveAttribute("data-state", "open");
      expect(list).not.toHaveClass("rt-sorting");
      expect(rows()[3]).toHaveTextContent("Extra model");
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
      dragToTop();
      pointer(document, "pointerup", 20);
      expect(rows()[0]).toHaveTextContent("Extra model");
      expect(screen.queryByRole("radiogroup")).not.toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
      const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
      expect(saved.members.map(({ modelId }) => modelId)).toEqual([
        "a-extra",
        "a-main",
        "b-main",
        "c-main",
      ]);
      expect(saved.defaultMemberId).toBe(saved.members[0].id);
      expect(saved.fallbackMemberIds).toEqual(
        saved.members.slice(1).map(({ id }) => id),
      );
    },
  );

  it.each(["templateCapability", "templateSpeed", "templateExpiry"] as const)(
    "removes the first model from %s and saves the remaining order",
    async (key) => {
      vi.mocked(intentRoutingApi.providers).mockResolvedValue(
        ["a", "b", "c"].map((id) => ({
          id,
          name: `Provider ${id}`,
          taskEligible: true,
          classifierEligible: false,
          models: [{ id: `${id}-main`, name: `${id} main`, outputPrice: 20 }],
        })),
      );
      mount();
      const create = screen.getByRole("button", {
        name: "intentRouting.create",
      });
      await waitFor(() => expect(create).toBeEnabled());
      drawTemplateSuggestions(create);
      const user = userEvent.setup();
      await user.click(
        screen.getByRole("button", { name: `routingPage.${key}` }),
      );
      const dialog = screen.getByRole("dialog", { name: `routingPage.${key}` });
      const rows = () => within(dialog).getAllByRole("listitem");
      expect(rows()).toHaveLength(3);
      await user.click(
        within(rows()[0]).getByRole("button", {
          name: /^routingPage.removeRuleModel /,
        }),
      );
      expect(rows()).toHaveLength(2);
      expect(rows()[0]).toHaveTextContent("b main");
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
      await user.click(
        within(dialog).getByRole("button", { name: "common.save" }),
      );
      await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
      const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
      expect(saved.members.map(({ modelId }) => modelId)).toEqual([
        "b-main",
        "c-main",
      ]);
      expect(saved.defaultMemberId).toBe(saved.members[0].id);
      expect(saved.fallbackMemberIds).toEqual([saved.members[1].id]);
    },
  );

  it("draws again on a new creation and saves the models shown in the chooser", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.providers).mockResolvedValue(
      Array.from({ length: 4 }, (_, index) => ({
        id: `supplier-${index}`,
        name: `Supplier ${index}`,
        taskEligible: true,
        classifierEligible: false,
        models: [{ id: `fixture-${index}`, name: `Model ${index}` }],
      })),
    );
    mount();
    const create = await screen.findByRole("button", {
      name: "intentRouting.create",
    });
    await waitFor(() => expect(create).toBeEnabled());
    const choose = (random: () => number, modelIds: string[]) => {
      drawTemplateSuggestions(create, random);
      const chooser = screen.getByRole("dialog", {
        name: "routingPage.templates",
      });
      const template = within(chooser).getByRole("button", {
        name: "routingPage.templateCapability",
      });
      expect(template.parentElement?.parentElement).toHaveTextContent(
        modelIds.join(" → "),
      );
      fireEvent.click(template);
    };
    choose(keepRankedOrder, ["fixture-0", "fixture-1", "fixture-2"]);
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(intentRoutingApi.save).not.toHaveBeenCalled();

    const drawn = ["fixture-1", "fixture-2", "fixture-3"];
    choose(() => 0, drawn);
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
    const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    expect(saved.members.map(({ modelId }) => modelId)).toEqual(drawn);
  });

  it("keeps the template picker compact while preserving ranked members", async () => {
    const models = Array.from({ length: 51 }, (_, index) => ({
      id: `model-${index + 1}`,
      name: `Model ${index + 1}`,
      maker: true,
      outputPrice: 20,
      released: new Date(Date.UTC(2026, 0, index + 1)).toISOString(),
    }));
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
        models,
      },
    ]);
    mount();
    const create = await screen.findByRole("button", {
      name: "intentRouting.create",
    });
    await waitFor(() => expect(create).toBeEnabled());
    drawTemplateSuggestions(create);
    fireEvent.click(
      screen.getByRole("button", { name: "routingPage.templateCapability" }),
    );
    expect(
      screen.getAllByRole("button", { name: /^Model \d+ · Fixture provider$/ }),
    ).toHaveLength(3);
    const order = within(
      screen.getByRole("list", { name: "routingPage.modelOrder" }),
    );
    const rows = () => order.getAllByRole("listitem");
    expect(order.getAllByRole("listitem")).toHaveLength(3);

    const grip = within(rows()[1]).getByRole("button", {
      name: "agentConnections.dragModel Model 50",
    });
    grip.focus();
    fireEvent.keyDown(grip, { key: "ArrowUp", altKey: true });
    expect(rows()[0]).toHaveTextContent("Model 50");
    expect(grip).toHaveFocus();

    fireEvent.click(
      screen.getByRole("button", {
        name: "routingPage.applyRecommendedModelOrder",
      }),
    );
    expect(rows()[0]).toHaveTextContent("Model 51");
    expect(rows()[2]).toHaveTextContent("Model 49");
    expect(intentRoutingApi.save).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
    const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    expect(saved.members.map((member) => member.modelId)).toEqual([
      "model-51",
      "model-50",
      "model-49",
    ]);
    expect(saved.defaultMemberId).toBe(saved.members[0].id);
    expect(saved.fallbackMemberIds).toEqual(
      saved.members.slice(1).map((member) => member.id),
    );
  });

  it("retains manual classifier entry for legacy catalogs without model listings", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: true,
      },
    ]);
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    await user.click(screen.getByText("routingPage.classifierAdvanced"));
    await user.click(
      screen.getByRole("combobox", {
        name: "intentRouting.classifierProvider",
      }),
    );
    await user.click(screen.getByRole("option", { name: "Fixture provider" }));
    fireEvent.change(screen.getByLabelText("intentRouting.classifierModel"), {
      target: { value: "fixture-model" },
    });
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          intentRecognitionEnabled: true,
          classifierTarget: {
            providerId: "provider",
            modelId: "fixture-model",
          },
        }),
      ),
    );
    expect(intentRoutingApi.trial).not.toHaveBeenCalled();
  });
  it("retains keyboard group ordering without menu entries", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, { ...group, id: "next", name: "Next group" }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const handle = await screen.findByRole("button", {
      name: "routingPage.arrangeGroup Fixture group",
    });
    fireEvent.keyDown(handle, { key: "ArrowUp", altKey: true });
    expect(intentRoutingApi.arrange).not.toHaveBeenCalled();
    expect(toast.success).not.toHaveBeenCalled();
    fireEvent.keyDown(handle, { key: "ArrowDown", altKey: true });
    await waitFor(() =>
      expect(intentRoutingApi.arrange).toHaveBeenCalledWith([
        "next",
        "fixture",
      ]),
    );
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledExactlyOnceWith(
        "routingPage.orderSaved",
      ),
    );
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it.each(["success", "save failure", "refresh failure"])(
    "keeps the dropped order visible during persistence and refresh: %s",
    async (outcome) => {
      const next = { ...group, id: "next", name: "Next group" };
      const catalog = {
        groups: [group, next],
        autoCreate: false,
        omittedGroups: 0,
      };
      vi.mocked(intentRoutingApi.list).mockResolvedValue(catalog);
      let finish!: () => void;
      vi.mocked(intentRoutingApi.arrange).mockImplementation(
        () =>
          new Promise<void>((resolve, reject) => {
            finish = () =>
              outcome === "save failure"
                ? reject("intentRouting.conflict")
                : resolve();
          }),
      );
      const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const { container } = mount(client);
      const handle = await screen.findByRole("button", {
        name: "routingPage.arrangeGroup Fixture group",
      });
      const rows = () =>
        Array.from(container.querySelectorAll("[data-routing-group]"));
      rows().forEach((row, index) => {
        vi.spyOn(row, "getBoundingClientRect").mockReturnValue({
          top: index * 60,
          bottom: (index + 1) * 60,
          height: 60,
        } as DOMRect);
      });
      const pointer = (type: string, y: number) => {
        const event = new MouseEvent(type, {
          bubbles: true,
          clientY: y,
          button: 0,
        });
        Object.defineProperty(event, "pointerId", { value: 7 });
        fireEvent(type === "pointerdown" ? handle : document, event);
      };
      const order = () =>
        rows().map((row) => row.getAttribute("data-routing-group"));
      pointer("pointerdown", 10);
      pointer("pointermove", 100);
      pointer("pointerup", 100);
      expect(order()).toEqual(["next", "fixture"]);
      expect(intentRoutingApi.arrange).toHaveBeenCalledExactlyOnceWith([
        "next",
        "fixture",
      ]);
      expect(toast.success).not.toHaveBeenCalled();
      // An unrelated cache refresh must not briefly restore the old server order.
      act(() =>
        client.setQueryData(["intent-routing-groups"], {
          ...catalog,
          groups: [{ ...group, name: "Updated fixture" }, next],
        }),
      );
      expect(order()).toEqual(["next", "fixture"]);
      await screen.findByText("Updated fixture");
      let finishRefresh!: () => void;
      vi.mocked(intentRoutingApi.list).mockImplementation(
        () =>
          new Promise((resolve, reject) => {
            finishRefresh = () =>
              outcome === "refresh failure"
                ? reject(new Error("Synthetic refresh failure"))
                : resolve({ ...catalog, groups: [next, group] });
          }),
      );
      await act(async () => finish());
      if (outcome === "save failure") {
        expect(toast.success).not.toHaveBeenCalled();
        expect(order()).toEqual(["fixture", "next"]);
        expect(screen.getByText("intentRouting.conflict")).toBeInTheDocument();
        expect(screen.getByText("Updated fixture")).toBeInTheDocument();
      } else {
        expect(toast.success).toHaveBeenCalledExactlyOnceWith(
          "routingPage.orderSaved",
        );
        expect(order()).toEqual(["next", "fixture"]);
        await act(async () => finishRefresh());
        await waitFor(() => expect(handle).toBeEnabled());
        expect(order()).toEqual(["next", "fixture"]);
        expect(toast.success).toHaveBeenCalledTimes(1);
      }
    },
  );

  it("changes a rule to an agent-scoped pause without retaining routing-only conditions", async () => {
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    await user.click(
      screen.getByRole("button", { name: "routingPage.ruleActions" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "routingPage.pauseRule" }),
    );
    expect(
      screen.queryByLabelText("intentRouting.ruleUseEffort"),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "intentRouting.addCondition" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "intentRouting.conditionAgents" }),
    );
    await user.click(
      screen.getByRole("button", { name: "intentRouting.conditionAgents" }),
    );
    await user.click(screen.getByRole("menuitemcheckbox", { name: "codex" }));
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          rules: [
            expect.objectContaining({
              pause: true,
              description: "",
              useEffort: "",
              fallbackMemberIds: [],
              conditions: { agents: ["codex"] },
            }),
          ],
        }),
      ),
    );
  });
  it("adds local AND conditions without turning on the intent classifier", async () => {
    mount();
    const user = userEvent.setup();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.click(
      screen.getByRole("button", {
        name: "intentRouting.removeCondition intentRouting.conditionIntent",
      }),
    );
    await user.click(
      screen.getByRole("button", { name: "intentRouting.addCondition" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "intentRouting.conditionTokens" }),
    );
    fireEvent.change(screen.getByLabelText("intentRouting.conditionTokens"), {
      target: { value: "4000" },
    });
    await user.click(
      screen.getByRole("button", { name: "intentRouting.addCondition" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "intentRouting.conditionImages" }),
    );
    await user.click(
      screen.getByRole("button", { name: "routingPage.ruleActions" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "intentRouting.ruleUseEffort" }),
    );
    await user.click(await screen.findByRole("menuitem", { name: "high" }));
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          intentRecognitionEnabled: false,
          rules: [
            expect.objectContaining({
              description: "",
              conditions: { tokens: 4000, images: true },
              useEffort: "high",
            }),
          ],
        }),
      ),
    );
  });
  it("keeps saved effort variants behind a single catalog chip", async () => {
    const members = [
      group.members[0],
      { ...group.members[0], id: "variant", effort: "high" },
    ];
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [{ ...group, members }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    expect(
      screen.getAllByRole("checkbox", { name: "fixture-model" }),
    ).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({ members }),
      ),
    );
  });
  it("searches and scrolls the rule model picker and saves fallbacks in selection order", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          members: [
            group.members[0],
            { ...group.members[0], id: "variant", effort: "none" },
            { ...group.members[0], id: "high-variant", effort: "high" },
            { ...group.members[0], id: "medium-variant", effort: "medium" },
          ],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.click(screen.getByText("intentRouting.advancedSettings"));
    const rule = screen.getByRole("group", {
      name: "intentRouting.ruleNumber",
    });
    expect(within(rule).getByText("routingPage.ruleConditions")).toBeVisible();
    expect(
      within(rule).getByRole("button", {
        name: /^intentRouting.target fixture-model/,
      }),
    ).toBeVisible();
    await user.click(
      within(rule).getByRole("button", { name: "routingPage.addRuleModel" }),
    );
    expectModelPickerWheelAllowed();
    const search = screen.getByRole("searchbox", {
      name: "modelPicker.search",
    });
    expect(search).toHaveFocus();
    await user.type(search, "high");
    expect(
      screen.queryByRole("radio", { name: /fixture-model :none/ }),
    ).toBeNull();
    await user.click(
      screen.getByRole("radio", { name: /fixture-model :high/ }),
    );
    await user.click(
      within(rule).getByRole("button", { name: "routingPage.addRuleModel" }),
    );
    await user.click(
      screen.getByRole("radio", { name: /fixture-model :none/ }),
    );
    const fallbacks = within(rule).getAllByRole("button", {
      name: /^routingPage.ruleFallbackModels /,
    });
    expect(fallbacks[0]).toHaveTextContent("fixture-model :high");
    expect(fallbacks[1]).toHaveTextContent("fixture-model :none");
    await user.click(fallbacks[0]);
    expect(
      screen.getByRole("radio", { name: /fixture-model :high/ }),
    ).toBeChecked();
    expect(
      screen.queryByRole("radio", { name: /fixture-model :none/ }),
    ).toBeNull();
    await user.keyboard("{Escape}");
    expect(fallbacks[0]).toHaveFocus();
    await user.click(fallbacks[0]);
    await user.click(
      screen.getByRole("radio", { name: /fixture-model :medium/ }),
    );
    const preferred = within(rule).getByRole("button", {
      name: /^intentRouting.target fixture-model/,
    });
    await user.click(preferred);
    expect(
      screen.getByRole("radio", { name: /^fixture-model ·/ }),
    ).toBeChecked();
    await user.click(
      screen.getByRole("radio", { name: /fixture-model :high/ }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalled());
    const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    expect(saved.rules[0].targetMemberId).toBe(saved.members[2].id);
    expect(saved.rules[0].fallbackMemberIds).toEqual([
      saved.members[3].id,
      saved.members[1].id,
    ]);
  });
  it("opens rule help by click or keyboard and returns focus on Escape", async () => {
    mount();
    const user = userEvent.setup();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.click(screen.getByText("intentRouting.advancedSettings"));
    const help = screen.getByRole("button", { name: "routingPage.ruleHelp" });
    const add = screen.getByRole("button", { name: "intentRouting.addRule" });
    expect(help.nextElementSibling).toBe(add);
    await user.click(help);
    const explanation = () =>
      screen.getByRole("dialog", { name: "routingPage.ruleHelp" });
    expect(explanation()).toHaveTextContent("intentRouting.conditionsHelp");
    await user.keyboard("{Escape}");
    expect(help).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(explanation()).toBeVisible();
  });
  it("focuses a new custom rule and keeps keyboard reordering available", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          members: [
            ...group.members,
            { ...group.members[0], id: "variant", effort: "high" },
          ],
          rules: [{ ...group.rules[0], enabled: false }],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.click(screen.getByText("intentRouting.advancedSettings"));
    expect(screen.getByText("routingPage.ruleDisabled")).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "intentRouting.addRule" }),
    );
    const [, added] = screen.getAllByRole("group", {
      name: "intentRouting.ruleNumber",
    });
    const description = within(added).getByRole("textbox", {
      name: "intentRouting.description",
    });
    await waitFor(() => expect(description).toHaveFocus());
    await user.type(description, "Write focused tests");
    const grip = within(added).getByRole("button", {
      name: "routingPage.moveRule",
    });
    grip.focus();
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    const [first] = screen.getAllByRole("group", {
      name: "intentRouting.ruleNumber",
    });
    expect(first).toBe(added);
    await waitFor(() => expect(grip).toHaveFocus());
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalled());
    const { rules } = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    expect(rules[0]).toMatchObject({
      description: "Write focused tests",
      enabled: true,
    });
    expect(rules[1]).toMatchObject({ ...group.rules[0], enabled: false });
  });
  it("recalculates pattern membership in an unsaved draft without adding the matches to its definition", async () => {
    vi.mocked(intentRoutingApi.matchedMembers).mockResolvedValue([
      { id: "match-future", providerId: "future", modelId: "new-model" },
    ]);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.change(screen.getByLabelText("intentRouting.matchPatterns"), {
      target: { value: "future/*" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.addPattern" }),
    );
    expect(await screen.findByText("future/new-model")).toBeInTheDocument();
    expect(intentRoutingApi.matchedMembers).toHaveBeenLastCalledWith(
      expect.objectContaining({ match: ["future/*"], members: group.members }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          match: ["future/*"],
          members: group.members,
        }),
      ),
    );
  });
  it("saves a pattern-only group without freezing its current catalog matches as explicit members", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          members: [],
          defaultMemberId: "",
          rules: [],
          match: ["provider/*"],
        },
      ],
      matchedMembers: {
        fixture: [
          { id: "derived", providerId: "provider", modelId: "fixture-model" },
        ],
      },
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.change(screen.getByLabelText("intentRouting.matchPatterns"), {
      target: { value: "re:^future/large$" },
    });
    expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          members: [],
          defaultMemberId: "",
          match: ["provider/*", "re:^future/large$"],
        }),
      ),
    );
  });
  it("saves real reference timeouts and member disablement without removing the member", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [{ ...group, off: ["member"] }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.change(screen.getByLabelText("intentRouting.firstToken"), {
      target: { value: "5" },
    });
    fireEvent.change(screen.getByLabelText("intentRouting.patience"), {
      target: { value: "20" },
    });
    fireEvent.click(
      screen.getByRole("radio", { name: "intentRouting.sinkBack" }),
    );
    fireEvent.click(
      screen.getByRole("radio", { name: "intentRouting.levelsNamed" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "max" }));
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          firstToken: 5,
          patience: 20,
          sink: true,
          levels: ["low", "medium", "high", "max"],
          off: ["member"],
          members: group.members,
        }),
      ),
    );
  });
  it.each([0, 1])(
    "shows management for a list with %i groups",
    async (count) => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: count ? [structuredClone(group)] : [],
        autoCreate: false,
        omittedGroups: 0,
      });
      mount();
      const manage = await screen.findByRole("button", {
        name: "routingPage.manage",
      });
      await waitFor(() =>
        expect(
          screen.getByRole("button", { name: "routingPage.refresh" }),
        ).toBeEnabled(),
      );
      if (!count) {
        expect(manage).toBeDisabled();
        expect(
          screen.queryByText("intentRouting.autoCreate"),
        ).not.toBeInTheDocument();
        expect(
          screen.getByRole("button", { name: "intentRouting.create" }),
        ).toBeEnabled();
        return;
      }
      expect(manage).toBeEnabled();
      fireEvent.click(manage);
      expect(
        screen.getByRole("checkbox", { name: "Fixture group" }),
      ).not.toBeChecked();
      expect(
        screen.getByRole("button", { name: "routingPage.removeSelected" }),
      ).toBeDisabled();
      fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
      expect(
        screen.queryByRole("checkbox", { name: "Fixture group" }),
      ).not.toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "routingPage.manage" }),
      ).toHaveAttribute("aria-pressed", "false");
      expect(intentRoutingApi.remove).not.toHaveBeenCalled();
    },
  );

  it("confirms bulk removal and uses each selected group's revision", async () => {
    let finishFirst!: () => void;
    let finishSecond!: () => void;
    const firstRemoval = new Promise<void>((resolve) => {
      finishFirst = resolve;
    });
    const secondRemoval = new Promise<void>((resolve) => {
      finishSecond = resolve;
    });
    vi.mocked(intentRoutingApi.remove)
      .mockReturnValueOnce(firstRemoval)
      .mockReturnValueOnce(secondRemoval);
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        structuredClone(group),
        {
          ...structuredClone(group),
          id: "second",
          name: "Second fixture",
          revision: 7,
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const manage = await screen.findByRole("button", {
      name: "routingPage.manage",
    });
    await waitFor(() => expect(manage).toBeEnabled());
    fireEvent.click(manage);
    fireEvent.click(
      screen.getByRole("checkbox", { name: "routingPage.selectAll" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "routingPage.removeSelected" }),
    );
    expect(intentRoutingApi.remove).not.toHaveBeenCalled();
    const confirmation = await screen.findByRole("dialog", {
      name: "routingPage.removeSelected",
    });
    fireEvent.click(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(intentRoutingApi.remove).not.toHaveBeenCalled();
    expect(
      screen.getByRole("checkbox", { name: "Fixture group" }),
    ).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "Second fixture" }),
    ).toBeChecked();
    fireEvent.click(
      screen.getByRole("button", { name: "routingPage.removeSelected" }),
    );
    const activeConfirmation = screen.getByRole("dialog", {
      name: "routingPage.removeSelected",
    });
    let finishRefresh!: (
      value: Awaited<ReturnType<typeof intentRoutingApi.list>>,
    ) => void;
    const refreshedGroups = new Promise<
      Awaited<ReturnType<typeof intentRoutingApi.list>>
    >((resolve) => {
      finishRefresh = resolve;
    });
    vi.mocked(intentRoutingApi.list).mockReturnValue(refreshedGroups);
    const confirm = within(activeConfirmation).getByRole("button", {
      name: "intentRouting.confirmDelete",
    });
    fireEvent.click(confirm);
    expect(
      screen.getByRole("dialog", { name: "routingPage.removeSelected" }),
    ).toBe(activeConfirmation);
    expect(confirm).toBeDisabled();
    expect(confirm).toHaveAttribute("aria-busy", "true");
    expect(confirm.querySelector("svg.animate-spin")).toBeInTheDocument();
    expect(
      within(activeConfirmation).getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    fireEvent.keyDown(activeConfirmation, { key: "Escape" });
    fireEvent.click(confirm);
    expect(
      screen.getByRole("dialog", { name: "routingPage.removeSelected" }),
    ).toBe(activeConfirmation);
    expect(intentRoutingApi.remove).toHaveBeenCalledTimes(1);
    finishFirst();
    await waitFor(() =>
      expect(intentRoutingApi.remove).toHaveBeenCalledTimes(2),
    );
    expect(confirm).toBeDisabled();
    finishSecond();
    await waitFor(() => expect(intentRoutingApi.list).toHaveBeenCalledTimes(2));
    expect(
      screen.queryByRole("dialog", { name: "routingPage.removeSelected" }),
    ).not.toBeInTheDocument();
    finishRefresh({ groups: [], autoCreate: false, omittedGroups: 0 });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(intentRoutingApi.remove).toHaveBeenCalledWith("fixture", 1);
    expect(intentRoutingApi.remove).toHaveBeenCalledWith("second", 7);
  });

  it("keeps bulk removal failures in the dialog and retries only the remaining groups", async () => {
    const second = {
      ...structuredClone(group),
      id: "second",
      name: "Second fixture",
      revision: 7,
    };
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, second],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.remove)
      .mockImplementationOnce(async () => {
        vi.mocked(intentRoutingApi.list).mockResolvedValue({
          groups: [second],
          autoCreate: false,
          omittedGroups: 0,
        });
      })
      .mockRejectedValueOnce(new Error("synthetic-private-removal-error"))
      .mockImplementationOnce(async () => {
        vi.mocked(intentRoutingApi.list).mockResolvedValue({
          groups: [],
          autoCreate: false,
          omittedGroups: 0,
        });
      });
    mount();
    const manage = await screen.findByRole("button", {
      name: "routingPage.manage",
    });
    await waitFor(() => expect(manage).toBeEnabled());
    fireEvent.click(manage);
    fireEvent.click(
      screen.getByRole("checkbox", { name: "routingPage.selectAll" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "routingPage.removeSelected" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "routingPage.removeSelected",
    });
    const confirm = within(dialog).getByRole("button", {
      name: "intentRouting.confirmDelete",
    });
    fireEvent.click(confirm);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    await waitFor(() => expect(confirm).toBeEnabled());
    expect(
      screen.getByRole("dialog", { name: "routingPage.removeSelected" }),
    ).toBe(dialog);
    expect(
      screen.queryByText("synthetic-private-removal-error"),
    ).not.toBeInTheDocument();
    expect(confirm.querySelector("svg.animate-spin")).not.toBeInTheDocument();
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(intentRoutingApi.remove).toHaveBeenCalledTimes(3);
    expect(intentRoutingApi.remove).toHaveBeenNthCalledWith(1, "fixture", 1);
    expect(intentRoutingApi.remove).toHaveBeenNthCalledWith(2, "second", 7);
    expect(intentRoutingApi.remove).toHaveBeenNthCalledWith(3, "second", 7);
  });

  it("keeps saved member order and default when the catalog is arranged differently", async () => {
    const members = [
      ...group.members,
      { id: "second", providerId: "provider", modelId: "second-model" },
    ];
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [{ ...group, members, fallbackMemberIds: ["second"] }],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: "second-model", name: "Second" },
          { id: "fixture-model", name: "First" },
        ],
      },
    ]);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          members,
          defaultMemberId: "member",
          fallbackMemberIds: ["second"],
        }),
      ),
    );
  });
  it("creates a group with expanded provider chips selected by default", async () => {
    let finishSave!: () => void;
    vi.mocked(intentRoutingApi.save).mockImplementation(
      (value) =>
        new Promise((resolve) => {
          finishSave = () => resolve({ ...value, revision: 1 });
        }),
    );
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: "one", name: "One" },
          { id: "two", name: "Two" },
        ],
      },
    ]);
    mount();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "intentRouting.create" }),
      ).toBeEnabled(),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.create" }),
    );
    chooseCustom();
    expect(document.querySelectorAll(".rt-model-chips")).not.toHaveLength(0);
    expect(
      screen.queryByRole("list", { name: "routingPage.modelOrder" }),
    ).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "New shared group" },
    });
    const provider = screen.getByRole("group", { name: "Fixture provider" });
    expect(
      within(provider).getByRole("checkbox", { name: "One" }),
    ).toBeChecked();
    expect(
      within(provider).getByRole("checkbox", { name: "Two" }),
    ).toBeChecked();
    expect(
      screen.queryByRole("button", { name: "intentRouting.addMember" }),
    ).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.selectNone" }),
    );
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.selectAll" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          enabled: true,
          members: [
            expect.objectContaining({ modelId: "one" }),
            expect.objectContaining({ modelId: "two" }),
          ],
        }),
      ),
    );
    const dialog = screen.getByRole("dialog", { name: "intentRouting.create" });
    expect(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    expect(
      within(dialog).getByRole("button", { name: "common.save" }),
    ).toBeDisabled();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "intentRouting.create" })).toBe(
      dialog,
    );
    expect(intentRoutingApi.save).toHaveBeenCalledOnce();
    finishSave();
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });
  it("toggles provider chips independently and reports partial selection", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: "one", name: "One" },
          { id: "two", name: "Two" },
        ],
      },
      {
        id: "backup",
        name: "Backup provider",
        taskEligible: true,
        classifierEligible: false,
        models: [{ id: "one", name: "One" }],
      },
      {
        id: "unavailable",
        name: "Unavailable provider",
        taskEligible: false,
        classifierEligible: false,
        models: [{ id: "blocked", name: "Blocked" }],
      },
    ]);
    mount();
    await screen.findByText("Fixture group");
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.create" }),
    );
    chooseCustom();
    const primary = within(
      screen.getByRole("group", { name: "Fixture provider" }),
    );
    const backup = within(
      screen.getByRole("group", { name: "Backup provider" }),
    );
    expect(primary.getByRole("checkbox", { name: "One" })).toBeChecked();
    expect(backup.getByRole("checkbox", { name: "One" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Blocked" })).toBeDisabled();
    fireEvent.click(primary.getByRole("checkbox", { name: "One" }));
    expect(
      primary.getByRole("checkbox", { name: "Fixture provider" }),
    ).toBePartiallyChecked();
    expect(backup.getByRole("checkbox", { name: "One" })).toBeChecked();
    fireEvent.click(
      primary.getByRole("checkbox", { name: "Fixture provider" }),
    );
    expect(primary.getByRole("checkbox", { name: "One" })).toBeChecked();
    fireEvent.click(
      primary.getByRole("checkbox", { name: "Fixture provider" }),
    );
    expect(primary.getByRole("checkbox", { name: "Two" })).not.toBeChecked();
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Only backup" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalled());
    expect(vi.mocked(intentRoutingApi.save).mock.calls[0][0].members).toEqual([
      expect.objectContaining({ providerId: "backup", modelId: "one" }),
    ]);
  });

  it("folds providers independently without changing selection and supports bulk changes while folded", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
        models: [{ id: "fixture-model", name: "First" }],
      },
      {
        id: "backup",
        name: "Backup provider",
        taskEligible: true,
        classifierEligible: false,
        models: [{ id: "backup-model", name: "Backup" }],
      },
    ]);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    const toggle = screen.getByRole("button", { name: "Fixture provider" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const controlled = document.getElementById(
      toggle.getAttribute("aria-controls")!,
    );
    expect(controlled).not.toHaveAttribute("inert");
    expect(toggle.lastElementChild).toHaveClass("rt-provider-chevron");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(controlled).toHaveAttribute("inert");
    expect(screen.queryByRole("checkbox", { name: "First" })).toBeNull();
    expect(screen.getByRole("checkbox", { name: "Backup" })).toBeVisible();
    expect(
      screen.getByRole("checkbox", { name: "Fixture provider" }),
    ).toBeChecked();
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.selectNone" }),
    );
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(
      screen.getByRole("checkbox", { name: "Fixture provider" }),
    ).not.toBeChecked();
    fireEvent.click(screen.getByRole("checkbox", { name: "Fixture provider" }));
    fireEvent.click(toggle);
    expect(screen.getByRole("checkbox", { name: "First" })).toBeChecked();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("shows only model names on chips even when context metadata is available", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: "fixture-model", name: "Million", contextWindow: 1000000 },
          { id: "medium", name: "Medium", contextWindow: 200000 },
          { id: "unknown", name: "Unknown" },
        ],
      },
    ]);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    const chip = screen
      .getByRole("checkbox", { name: "Million" })
      .closest("label")!;
    expect(chip).toHaveTextContent(/^Million$/);
    expect(
      screen.getByRole("checkbox", { name: "Medium" }).closest("label")!,
    ).toHaveTextContent(/^Medium$/);
    expect(
      screen.getByRole("checkbox", { name: "Unknown" }).closest("label")!,
    ).toHaveTextContent(/^Unknown$/);
    expect(document.querySelector(".rt-model-context")).toBeNull();
  });

  it("preserves saved member settings and rule targets when selecting more catalog models", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          members: [{ ...group.members[0], effort: "high", free: true }],
          off: ["member"],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: "fixture-model", name: "Fixture model" },
          { id: "extra", name: "Extra" },
        ],
      },
    ]);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    expect(
      screen.getByRole("checkbox", { name: "Fixture model" }),
    ).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Extra" })).not.toBeChecked();
    fireEvent.click(
      screen.getByRole("button", { name: "providerEditor.selectAll" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          members: [
            { ...group.members[0], effort: "high", free: true },
            expect.objectContaining({ modelId: "extra" }),
          ],
          defaultMemberId: "member",
          off: ["member"],
          rules: group.rules,
        }),
      ),
    );
  });

  it("saves custom group capacity independently of the routing strategy", async () => {
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    await user.click(
      screen.getByRole("radio", { name: "intentRouting.contextCustom" }),
    );
    fireEvent.change(screen.getByLabelText("intentRouting.contextTokens"), {
      target: { value: "500000" },
    });
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({ contextWindow: 500000 }),
      ),
    );
  });
  it("keeps empty group guidance independent of batch management", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: true,
      omittedGroups: 0,
      contextSettings: { fullWindow: false },
    });
    mount();
    expect(await screen.findByText("routingPage.noGroups")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "routingPage.manage" }),
    ).toBeDisabled();
    expect(
      screen.queryByText("intentRouting.autoCreate"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "intentRouting.create" }),
    ).toBeEnabled();
    expect(
      screen.queryByLabelText("intentRouting.fullWindow"),
    ).not.toBeInTheDocument();
  });

  const scrollDescriptor = Object.getOwnPropertyDescriptor(
    Element.prototype,
    "scrollIntoView",
  );
  beforeAll(() => {
    Object.defineProperty(Element.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
  });
  afterAll(() => {
    if (scrollDescriptor)
      Object.defineProperty(
        Element.prototype,
        "scrollIntoView",
        scrollDescriptor,
      );
    else Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  });
  it("renders compact rows and filters by provider or model without changing groups", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        group,
        {
          ...group,
          id: "other",
          name: "Other group",
          members: [
            {
              id: "other",
              providerId: "missing-provider",
              modelId: "other-model",
            },
          ],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const list = await screen.findByRole("rowgroup", {
      name: "intentRouting.title",
    });
    expect(list.children).toHaveLength(2);
    expect(
      screen.getByText("Fixture provider · fixture-model"),
    ).toBeInTheDocument();
    const search = screen.getByRole("searchbox", {
      name: "intentRouting.filter",
    });
    fireEvent.change(search, { target: { value: "FIXTURE PROVIDER" } });
    expect(list.children).toHaveLength(1);
    fireEvent.change(search, { target: { value: "other-model" } });
    expect(screen.getByText("Other group")).toBeInTheDocument();
    expect(screen.queryByText("Fixture group")).not.toBeInTheDocument();
    fireEvent.change(search, { target: { value: "no-match" } });
    expect(screen.getByText("intentRouting.noMatches")).toBeInTheDocument();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("edits a group in a dialog while keeping the compact list in place", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        { ...group, id: "before", name: "Before group" },
        group,
        { ...group, id: "after", name: "After group" },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const list = await screen.findByRole("rowgroup", {
      name: "intentRouting.title",
    });
    fireEvent.click(
      within(list.children[1] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    const editor = screen.getByRole("form", { name: "Fixture group" });
    const dialog = screen.getByRole("dialog", { name: "Fixture group" });
    expect(dialog).toContainElement(editor);
    expect(list.children).toHaveLength(3);
    expect(list).not.toContainElement(editor);
    expect(list.children[0]).toHaveTextContent("Before group");
    expect(list.children[1]).toHaveTextContent("Fixture group");
    expect(list.children[2]).toHaveTextContent("After group");
    expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
    expect(screen.getByLabelText("intentRouting.name")).toHaveFocus();
    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(list.children).toHaveLength(3);
    await waitFor(() =>
      expect(
        within(list.children[1] as HTMLElement).getByRole("button", {
          name: "common.edit",
        }),
      ).toHaveFocus(),
    );
    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("opens a new group in a dialog and preserves its draft after a failed save", async () => {
    vi.mocked(intentRoutingApi.save).mockRejectedValue(
      "synthetic-private-error",
    );
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: "one", name: "One" },
          { id: "two", name: "Two" },
        ],
      },
    ]);
    mount();
    await screen.findByText("Fixture group");
    const list = screen.getByRole("rowgroup", { name: "intentRouting.title" });
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.create" }),
    );
    chooseCustom();
    const dialog = screen.getByRole("dialog", { name: "intentRouting.create" });
    const editor = screen.getByRole("form", { name: "intentRouting.create" });
    expect(dialog).toContainElement(editor);
    expect(list).not.toContainElement(editor);
    expect(list.children).toHaveLength(1);
    expect(screen.getByLabelText("intentRouting.name")).toHaveFocus();
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "routingPage.templateCustom",
    );
    expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Changed draft" },
    });
    expect(screen.getByRole("checkbox", { name: "One" })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Changed draft",
    );
    expect(screen.getByRole("dialog", { name: "intentRouting.create" })).toBe(
      dialog,
    );
    expect(list.children).toHaveLength(1);
    expect(
      screen.queryByText("synthetic-private-error"),
    ).not.toBeInTheDocument();
  });

  it.each(["cancel", "escape"])(
    "closes a new group with %s and restores focus without saving",
    async (method) => {
      const user = userEvent.setup();
      mockTwoTaskModels();
      mount();
      await screen.findByText("Fixture group");
      const trigger = screen.getByRole("button", {
        name: "intentRouting.create",
      });
      await user.click(trigger);
      await user.click(
        screen.getByRole("button", { name: "routingPage.templateCustom" }),
      );
      const dialog = screen.getByRole("dialog", {
        name: "intentRouting.create",
      });
      await user.type(
        screen.getByLabelText("intentRouting.name"),
        "Unsaved group",
      );
      if (method === "cancel") {
        await user.click(
          within(dialog).getByRole("button", { name: "common.cancel" }),
        );
      } else {
        await user.keyboard("{Escape}");
      }
      await waitFor(() =>
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
      );
      expect(trigger).toHaveFocus();
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
      await user.click(trigger);
      await user.click(
        screen.getByRole("button", { name: "routingPage.templateCustom" }),
      );
      expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
        "routingPage.templateCustom",
      );
    },
  );

  it("opens a dialog from the page action without a duplicate create button", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const view = render(
      <QueryClientProvider client={client}>
        <IntentRoutingPanel createRequest={0} showCreateButton={false} />
      </QueryClientProvider>,
    );
    await screen.findByText("Fixture group");
    expect(
      screen.queryByRole("button", { name: "intentRouting.create" }),
    ).not.toBeInTheDocument();
    view.rerender(
      <QueryClientProvider client={client}>
        <IntentRoutingPanel createRequest={1} showCreateButton={false} />
      </QueryClientProvider>,
    );
    const chooser = await screen.findByRole("dialog", {
      name: "routingPage.templates",
    });
    expect(
      within(chooser).getByRole("button", {
        name: "routingPage.templateCustom",
      }),
    ).toBeEnabled();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled();
  });

  it("preserves the list filter after cancelling an edited group", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, { ...group, id: "other", name: "Other group" }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const list = await screen.findByRole("rowgroup", {
      name: "intentRouting.title",
    });
    const filter = screen.getByRole("searchbox", {
      name: "intentRouting.filter",
    });
    fireEvent.change(filter, { target: { value: "Fixture group" } });
    fireEvent.click(
      within(list.children[0] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    const dialog = screen.getByRole("dialog", { name: "Fixture group" });
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Unsaved name" },
    });
    expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Unsaved name",
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    expect(filter).toHaveValue("Fixture group");
    expect(
      screen.getByRole("heading", { name: "Fixture group" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Other group")).not.toBeInTheDocument();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("confirms discarding changed drafts before editing another group or creating one", async () => {
    mockTwoTaskModels();
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, { ...group, id: "other", name: "Other group" }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const list = await screen.findByRole("rowgroup", {
      name: "intentRouting.title",
    });
    fireEvent.click(
      within(list.children[0] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Unsaved name" },
    });
    const otherEdit = within(list.children[1] as HTMLElement).getByRole(
      "button",
      {
        name: "common.edit",
        hidden: true,
      },
    );
    fireEvent.click(otherEdit);
    let dialog = screen.getByRole("dialog", {
      name: "intentRouting.discardTitle",
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Unsaved name",
    );
    fireEvent.click(otherEdit);
    dialog = screen.getByRole("dialog", { name: "intentRouting.discardTitle" });
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "intentRouting.discardChanges",
      }),
    );
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Other group",
    );
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Another unsaved name" },
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "intentRouting.create",
        hidden: true,
      }),
    );
    chooseCustom();
    dialog = screen.getByRole("dialog", { name: "intentRouting.discardTitle" });
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "intentRouting.discardChanges",
      }),
    );
    expect(
      screen.getByRole("form", { name: "intentRouting.create" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "routingPage.templateCustom",
    );
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("opens a dialog from the row and dismisses it with Escape", async () => {
    mount();
    fireEvent.click(
      await screen.findByRole("heading", { name: "Fixture group" }),
    );
    const editor = screen.getByRole("form", { name: "Fixture group" });
    expect(
      within(editor).queryByText("intentRouting.foldEditor"),
    ).not.toBeInTheDocument();
    expect(
      within(editor).queryByRole("button", { name: "common.edit" }),
    ).not.toBeInTheDocument();
    const dialog = screen.getByRole("dialog", { name: "Fixture group" });
    expect(dialog).toContainElement(editor);
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it.each(["name", "summary", "blank"] as const)(
    "opens from the row's %s inside the focusable settings tab",
    async (area) => {
      mount();
      const heading = await screen.findByRole("heading", {
        name: "Fixture group",
      });
      const row = heading.closest("[data-routing-group]") as HTMLElement;
      const targets = {
        name: heading,
        summary: within(row).getByText("Fixture provider · fixture-model"),
        icon: within(row).getByLabelText("intentRouting.globalResource"),
        blank: row,
      };
      fireEvent.click(targets[area]);
      expect(
        screen.getByRole("dialog", { name: "Fixture group" }),
      ).toContainElement(screen.getByRole("form", { name: "Fixture group" }));
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
    },
  );

  it("opens removal from Actions and keeps only Cancel/Save in the editor footer", async () => {
    mount();
    const user = userEvent.setup();
    const heading = await screen.findByRole("heading", {
      name: "Fixture group",
    });
    const actions = within(heading.closest("tr")!).getAllByRole("cell").at(-1)!;
    const remove = within(actions).getByRole("button", {
      name: "common.delete",
    });
    expect(remove).toHaveTextContent(/^$/);
    expect(remove).toHaveAttribute("title", "common.delete");
    await user.click(remove);
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    const confirmation = screen.getByRole("dialog", { name: "common.delete" });
    expect(confirmation).toHaveTextContent("intentRouting.deletePrompt");
    expect(intentRoutingApi.remove).not.toHaveBeenCalled();
    await user.click(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(remove).toHaveFocus();
    await user.click(
      within(actions).getByRole("button", { name: "common.edit" }),
    );
    const editor = screen.getByRole("dialog", { name: "Fixture group" });
    const footer = within(
      editor.querySelector(".vary-dialog-footer")! as HTMLElement,
    );
    expect(
      footer.queryByRole("button", { name: "common.delete" }),
    ).not.toBeInTheDocument();
    expect(
      footer.getAllByRole("button").map((button) => button.textContent),
    ).toEqual(["common.cancel", "common.save"]);
    expect(intentRoutingApi.remove).not.toHaveBeenCalled();
  });

  it("confirms row deletion with the saved revision and prevents dismissal while pending", async () => {
    let finish!: () => void;
    vi.mocked(intentRoutingApi.remove).mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.delete" }),
    );
    const confirmation = screen.getByRole("dialog", { name: "common.delete" });
    await user.click(
      within(confirmation).getByRole("button", {
        name: "intentRouting.confirmDelete",
      }),
    );
    expect(intentRoutingApi.remove).toHaveBeenCalledWith("fixture", 1);
    expect(
      within(confirmation).getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    expect(
      within(confirmation).getByRole("button", {
        name: "intentRouting.confirmDelete",
      }),
    ).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(confirmation).toBeInTheDocument();
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: false,
      omittedGroups: 0,
    });
    await act(async () => {
      finish();
    });
    await waitFor(() =>
      expect(
        screen.queryByRole("heading", { name: "Fixture group" }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("preserves routing and free status on edit", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          routing: "fastest",
          members: [{ ...group.members[0], free: true }],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    expect(screen.getByText("routingPage.templateSpeedRuleHint")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          routing: "fastest",
          members: [expect.objectContaining({ free: true })],
        }),
      ),
    );
  });

  it("does not report unavailable members before the provider catalog loads", async () => {
    let finish!: (
      value: Awaited<ReturnType<typeof intentRoutingApi.providers>>,
    ) => void;
    vi.mocked(intentRoutingApi.providers).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    mount();
    await screen.findByText("Fixture group");
    expect(
      screen.queryByLabelText("intentRouting.noMemberReady"),
    ).not.toBeInTheDocument();
    await act(async () => finish([]));
    expect(
      await screen.findByLabelText("intentRouting.noMemberReady"),
    ).toBeVisible();
  });

  it.each(["ordered", "fastest"] as const)(
    "keeps a %s group ready when only some members are unavailable",
    async (routing) => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [
          {
            ...group,
            routing,
            members: [
              ...group.members,
              { id: "missing", providerId: "gone", modelId: "gone-model" },
            ],
          },
        ],
        autoCreate: false,
        omittedGroups: 0,
      });
      mount();
      await screen.findByText("Fixture group");
      await waitFor(() =>
        expect(intentRoutingApi.providers).toHaveBeenCalled(),
      );
      expect(
        screen.queryByLabelText("intentRouting.memberNotReady"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByLabelText("intentRouting.noMemberReady"),
      ).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
      const missing = screen.getByRole("group", { name: "gone" });
      expect(
        within(missing).getByText(
          "intentRouting.memberUnavailable.providerMissing",
        ),
      ).toBeVisible();
      expect(
        within(missing).getByRole("checkbox", { name: "gone-model" }),
      ).toBeChecked();
    },
  );

  it.each([
    { members: [], matched: [], routing: "ordered" as const, off: [] },
    {
      members: group.members,
      matched: [],
      routing: "fastest" as const,
      off: ["member"],
    },
    {
      members: [{ id: "missing", providerId: "gone", modelId: "gone-model" }],
      matched: [],
      routing: "ordered" as const,
      off: [],
    },
  ])(
    "warns when no participating member is ready: %j",
    async ({ members, matched, routing, off }) => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [{ ...group, members, routing, off }],
        matchedMembers: { [group.id]: matched },
        autoCreate: false,
        omittedGroups: 0,
      });
      mount();
      expect(
        await screen.findByLabelText("intentRouting.noMemberReady"),
      ).toBeVisible();
    },
  );

  it.each(["ordered", "fastest"] as const)(
    "counts pattern matches in %s group readiness",
    async (routing) => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [
          {
            ...group,
            routing,
            members: [
              { id: "missing", providerId: "gone", modelId: "gone-model" },
            ],
          },
        ],
        matchedMembers: {
          [group.id]: [{ ...group.members[0], id: "dynamic" }],
        },
        autoCreate: false,
        omittedGroups: 0,
      });
      mount();
      await screen.findByText("Fixture group");
      await waitFor(() =>
        expect(intentRoutingApi.providers).toHaveBeenCalled(),
      );
      expect(
        screen.queryByLabelText("intentRouting.memberNotReady"),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByLabelText("intentRouting.noMemberReady"),
      ).not.toBeInTheDocument();
    },
  );

  it.each(["cancel", "escape"])(
    "dismisses existing group changes with %s and restores focus without saving",
    async (method) => {
      const user = userEvent.setup();
      mount();
      const trigger = await screen.findByRole("button", {
        name: "common.edit",
      });
      await user.click(trigger);
      const dialog = screen.getByRole("dialog", { name: "Fixture group" });
      fireEvent.change(screen.getByLabelText("intentRouting.name"), {
        target: { value: "Unsaved name" },
      });
      if (method === "cancel") {
        await user.click(
          within(dialog).getByRole("button", { name: "common.cancel" }),
        );
      } else {
        await user.keyboard("{Escape}");
      }
      await waitFor(() =>
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
      );
      expect(trigger).toHaveFocus();
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
      await user.click(trigger);
      expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
        "Fixture group",
      );
    },
  );

  it("keeps an existing dialog open while saving and retains the draft on failure", async () => {
    let rejectSave!: (reason: unknown) => void;
    vi.mocked(intentRoutingApi.save).mockReturnValueOnce(
      new Promise((_, reject) => {
        rejectSave = reject;
      }),
    );
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    const dialog = screen.getByRole("dialog", { name: "Fixture group" });
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Edited group" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.save" }),
    );
    await waitFor(() => expect(dialog).toHaveAttribute("aria-busy", "true"));
    expect(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "Fixture group" })).toBe(dialog);
    rejectSave(new Error("synthetic-private-save-error"));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Edited group",
    );
    expect(
      screen.queryByText("synthetic-private-save-error"),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", { name: "common.save" }),
    ).toBeEnabled();
  });

  it("closes the saved dialog without locking other rows during its refresh", async () => {
    const other = { ...group, id: "other", name: "Other group" };
    let finishRefresh!: () => void;
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, other],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.save).mockImplementation(async (value) => {
      const saved = { ...value, revision: value.revision + 1 };
      vi.mocked(intentRoutingApi.list).mockReturnValue(
        new Promise((resolve) => {
          finishRefresh = () =>
            resolve({
              groups: [saved, other],
              autoCreate: false,
              omittedGroups: 0,
            });
        }),
      );
      return saved;
    });
    mount();
    const list = await screen.findByRole("rowgroup", {
      name: "intentRouting.title",
    });
    const otherPublication = within(list.children[1] as HTMLElement).getByRole(
      "switch",
    );
    const otherArrange = within(list.children[1] as HTMLElement).getByRole(
      "button",
      {
        name: "routingPage.arrangeGroup Other group",
      },
    );
    fireEvent.click(
      within(list.children[0] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    expect(otherPublication).toBeDisabled();
    expect(otherArrange).toBeDisabled();
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Saved group" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(finishRefresh).toBeTypeOf("function"));
    expect(otherPublication).toBeEnabled();
    expect(otherArrange).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "routingPage.refresh" }),
    ).toBeEnabled();
    expect(
      within(list.children[0] as HTMLElement).getByRole("switch"),
    ).toBeEnabled();
    finishRefresh();
    await screen.findByText("Saved group");
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(list.children).toHaveLength(2);
    expect(
      within(list.children[0] as HTMLElement).getByText("Saved group"),
    ).toBeInTheDocument();
    expect(
      within(list.children[1] as HTMLElement).getByRole("switch"),
    ).toBeEnabled();
    await waitFor(() =>
      expect(
        within(list.children[0] as HTMLElement).getByRole("button", {
          name: "common.edit",
        }),
      ).toHaveFocus(),
    );
  });

  it("does not show a late plan result after closing a dialog and editing another group", async () => {
    let finish!: (
      value: Awaited<ReturnType<typeof intentRoutingApi.preview>>,
    ) => void;
    vi.mocked(intentRoutingApi.preview).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, { ...group, id: "other", name: "Other group" }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const list = await screen.findByRole("rowgroup", {
      name: "intentRouting.title",
    });
    fireEvent.click(
      within(list.children[0] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.preview" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(
      within(list.children[1] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Other group",
    );
    finish({
      groupId: group.id,
      revision: 1,
      reason: "default_target",
      live: false,
      candidates: [
        { id: "late", providerId: "provider", modelId: "late-model" },
      ],
    });
    await waitFor(() =>
      expect(screen.queryByText("late-model")).not.toBeInTheDocument(),
    );
  });

  it("hides aliases while preserving alias search and an existing group configuration", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          modelAlias: "auto/fixture-model",
          routing: "ordered",
          autoGenerated: true,
        },
      ],
      autoCreate: true,
      omittedGroups: 0,
    });
    mount();
    await screen.findByText("Fixture group");
    expect(screen.queryByText("auto/fixture-model")).not.toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("searchbox", { name: "intentRouting.filter" }),
      { target: { value: "auto/fixture-model" } },
    );
    expect(screen.getByText("Fixture group")).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "common.edit" }));
    await user.type(screen.getByLabelText("intentRouting.name"), " updated");
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "fixture",
          modelAlias: "auto/fixture-model",
          routing: "ordered",
          name: "Fixture group updated",
          members: group.members,
        }),
      ),
    );
  });

  it("omits the explanatory block and its disclosure entry", async () => {
    mount();
    await screen.findByText("Fixture group");
    for (const key of [
      "draftSummary",
      "details",
      "draftNotice",
      "eligibility",
      "autoCreateDescription",
      "autoCatalogHelp",
    ]) {
      expect(
        screen.queryByText(`intentRouting.${key}`),
      ).not.toBeInTheDocument();
    }
    expect(
      screen.getByRole("rowgroup", { name: "intentRouting.title" }),
    ).toBeInTheDocument();
  });

  it("refreshes the cached group list without starting upstream catalog synchronization", async () => {
    vi.mocked(intentRoutingApi.refreshCatalogs).mockReturnValue(
      new Promise(() => {}),
    );
    mount();
    const reload = await screen.findByRole("button", {
      name: "routingPage.refresh",
    });
    await waitFor(() => expect(reload).toBeEnabled());
    vi.mocked(intentRoutingApi.list).mockClear();
    fireEvent.click(reload);
    await waitFor(() => expect(intentRoutingApi.list).toHaveBeenCalledOnce());
    await waitFor(() => expect(reload).toHaveAttribute("aria-busy", "false"));
    expect(intentRoutingApi.refreshCatalogs).not.toHaveBeenCalled();
    expect(toast.success).toHaveBeenCalledExactlyOnceWith(
      "routingPage.refreshSuccess",
    );
  });

  it("keeps cached refresh and group editing available during explicit model sync", async () => {
    let finish!: (value: []) => void;
    vi.mocked(intentRoutingApi.refreshCatalogs).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    mount();
    const sync = await screen.findByRole("button", {
      name: "routingPage.syncModels",
    });
    await waitFor(() => expect(sync).toBeEnabled());
    fireEvent.click(sync);
    fireEvent.click(sync);
    expect(intentRoutingApi.refreshCatalogs).toHaveBeenCalledOnce();
    expect(sync).toHaveAttribute("aria-busy", "true");
    const reload = screen.getByRole("button", { name: "routingPage.refresh" });
    expect(reload).toBeEnabled();
    vi.mocked(intentRoutingApi.list).mockClear();
    fireEvent.click(reload);
    await waitFor(() => expect(intentRoutingApi.list).toHaveBeenCalledOnce());
    await waitFor(() => expect(reload).toHaveAttribute("aria-busy", "false"));
    expect(sync).toHaveAttribute("aria-busy", "true");
    fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Unsaved during sync" },
    });
    await act(async () => finish([]));
    await waitFor(() => expect(sync).toHaveAttribute("aria-busy", "false"));
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Unsaved during sync",
    );
  });

  it.each([true, false])(
    "keeps controls and new drafts available during refresh (empty list: %s)",
    async (empty) => {
      const snapshot = {
        groups: empty ? [] : [structuredClone(group)],
        autoCreate: false,
        omittedGroups: 0,
      };
      vi.mocked(intentRoutingApi.list).mockResolvedValue(snapshot);
      mockTwoTaskModels();
      let finishList!: (value: typeof snapshot) => void;
      mount();
      const reload = await screen.findByRole("button", {
        name: "routingPage.refresh",
      });
      await waitFor(() => expect(reload).toBeEnabled());
      vi.mocked(intentRoutingApi.list).mockReturnValueOnce(
        new Promise((resolve) => {
          finishList = resolve;
        }),
      );
      fireEvent.click(reload);
      expect(reload).toHaveAttribute("aria-busy", "true");
      expect(reload.querySelector(".rt-refresh-icon")).toHaveClass(
        "animate-spin",
      );
      expect(reload.querySelector(".lucide-loader-circle")).toBeInTheDocument();
      expect(reload.querySelector(".lucide-refresh-cw")).toBeNull();
      expect(reload).toBeDisabled();
      expect(
        screen
          .getByRole("button", { name: "routingPage.manage" })
          .hasAttribute("disabled"),
      ).toBe(empty);
      const create = screen.getByRole("button", {
        name: "intentRouting.create",
      });
      expect(create).toBeEnabled();
      fireEvent.click(reload);
      expect(intentRoutingApi.refreshCatalogs).not.toHaveBeenCalled();
      fireEvent.click(create);
      chooseCustom();
      const name = screen.getByLabelText("intentRouting.name");
      expect(name).toBeEnabled();
      fireEvent.change(name, { target: { value: "Draft during refresh" } });
      expect(reload).toHaveAttribute("aria-busy", "true");
      expect(name).toBeEnabled();
      await act(async () => finishList(snapshot));
      await waitFor(() => expect(reload).toHaveAttribute("aria-busy", "false"));
      expect(reload.querySelector(".rt-refresh-icon")).not.toHaveClass(
        "animate-spin",
      );
      expect(reload.querySelector(".lucide-loader-circle")).toBeNull();
      expect(reload.querySelector(".lucide-refresh-cw")).toBeInTheDocument();
      expect(name).toHaveValue("Draft during refresh");
    },
  );

  it("reports partial model sync without losing cached groups", async () => {
    vi.mocked(intentRoutingApi.refreshCatalogs).mockResolvedValue([
      { providerId: "updated", status: "updated", modelCount: 2 },
      {
        providerId: "slow",
        status: "cached",
        modelCount: 3,
        reason: "timeout",
      },
      {
        providerId: "missing",
        status: "unavailable",
        modelCount: 0,
        reason: "timeout",
      },
    ]);
    mount();
    const sync = await screen.findByRole("button", {
      name: "routingPage.syncModels",
    });
    await waitFor(() => expect(sync).toBeEnabled());
    fireEvent.click(sync);
    await waitFor(() =>
      expect(toast.message).toHaveBeenCalledWith(
        "routingPage.syncModelsResult",
      ),
    );
    await waitFor(() => expect(sync).toHaveAttribute("aria-busy", "false"));
    expect(screen.getByText("Fixture group")).toBeVisible();
    expect(intentRoutingApi.setAutoCreate).not.toHaveBeenCalled();
  });

  it("ends refresh loading after a safe error and allows retry", async () => {
    vi.mocked(intentRoutingApi.refreshCatalogs)
      .mockRejectedValueOnce("synthetic-private-refresh-error")
      .mockResolvedValue([]);
    mount();
    const reload = await screen.findByRole("button", {
      name: "routingPage.syncModels",
    });
    await waitFor(() => expect(reload).toBeEnabled());
    fireEvent.click(reload);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    await waitFor(() => expect(reload).toHaveAttribute("aria-busy", "false"));
    expect(reload).toBeEnabled();
    expect(screen.queryByText("synthetic-private-refresh-error")).toBeNull();
    fireEvent.click(reload);
    await waitFor(() =>
      expect(intentRoutingApi.refreshCatalogs).toHaveBeenCalledTimes(2),
    );
    await waitFor(() => expect(reload).toBeEnabled());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("does not unlock an active save when a concurrent refresh finishes", async () => {
    let finishCatalog!: (value: []) => void;
    let failSave!: (reason: string) => void;
    vi.mocked(intentRoutingApi.refreshCatalogs).mockReturnValueOnce(
      new Promise((resolve) => {
        finishCatalog = resolve;
      }),
    );
    vi.mocked(intentRoutingApi.save).mockReturnValueOnce(
      new Promise((_, reject) => {
        failSave = reject;
      }),
    );
    mount();
    const reload = await screen.findByRole("button", {
      name: "routingPage.syncModels",
    });
    await waitFor(() => expect(reload).toBeEnabled());
    fireEvent.click(reload);
    const edit = screen.getByRole("button", { name: "common.edit" });
    expect(edit).toBeEnabled();
    fireEvent.click(edit);
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Concurrent edit" },
    });
    const dialog = screen.getByRole("dialog", { name: "Fixture group" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.save" }),
    );
    await waitFor(() => expect(dialog).toHaveAttribute("aria-busy", "true"));
    await act(async () => finishCatalog([]));
    await waitFor(() => expect(reload).toHaveAttribute("aria-busy", "false"));
    expect(dialog).toHaveAttribute("aria-busy", "true");
    expect(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    await act(async () => failSave("intentRouting.conflict"));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "intentRouting.conflict",
    );
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Concurrent edit",
    );
  });

  it.each([true, false])(
    "refreshes catalogs without listing provider status rows with autoCreate=%s",
    async (autoCreate) => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [],
        autoCreate,
        omittedGroups: 0,
      });
      vi.mocked(intentRoutingApi.refreshCatalogs).mockResolvedValue([
        { providerId: "provider", status: "cached", modelCount: 3 },
      ]);
      mount();
      const reload = await screen.findByRole("button", {
        name: "routingPage.syncModels",
      });
      await waitFor(() => expect(reload).toBeEnabled());
      fireEvent.click(reload);
      await waitFor(() =>
        expect(intentRoutingApi.refreshCatalogs).toHaveBeenCalledOnce(),
      );
      await waitFor(() => expect(reload).toBeEnabled());
      expect(
        screen.queryByText(/intentRouting.catalogStatus.cached/),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByText("intentRouting.emptyAuto"),
      ).not.toBeInTheDocument();
      expect(screen.queryByText("intentRouting.empty")).not.toBeInTheDocument();
    },
  );

  it("retains saved discovery when a manual catalog refresh fails", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group],
      autoCreate: true,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.refreshCatalogs).mockRejectedValue(
      "synthetic-private-error",
    );
    mount();
    await screen.findByText("Fixture group");
    fireEvent.click(
      screen.getByRole("button", { name: "routingPage.syncModels" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    expect(
      screen.queryByText("synthetic-private-error"),
    ).not.toBeInTheDocument();
    expect(intentRoutingApi.setAutoCreate).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("switch", { name: "intentRouting.autoCreate" }),
    ).not.toBeInTheDocument();
  });

  it("does not reuse a refresh result after the panel unmounts", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: true,
      omittedGroups: 0,
    });
    let finish!: (
      value: Awaited<ReturnType<typeof intentRoutingApi.refreshCatalogs>>,
    ) => void;
    vi.mocked(intentRoutingApi.refreshCatalogs).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const panel = mount();
    const reload = await screen.findByRole("button", {
      name: "routingPage.syncModels",
    });
    await waitFor(() => expect(reload).toBeEnabled());
    fireEvent.click(reload);
    await waitFor(() =>
      expect(intentRoutingApi.refreshCatalogs).toHaveBeenCalledOnce(),
    );
    panel.unmount();
    mount();
    finish([{ providerId: "provider", status: "updated", modelCount: 1 }]);
    await screen.findByText("routingPage.refresh");
    expect(
      screen.queryByText(/intentRouting.catalogStatus.updated/),
    ).not.toBeInTheDocument();
  });

  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [structuredClone(group)],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.setAutoCreate).mockResolvedValue(undefined);
    vi.mocked(intentRoutingApi.refreshCatalogs).mockResolvedValue([]);
    vi.mocked(intentRoutingApi.discoverCatalogs).mockResolvedValue([]);
    vi.mocked(intentRoutingApi.autoGroupSkip).mockResolvedValue([]);
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
      },
    ]);
    vi.mocked(intentRoutingApi.save).mockImplementation(async (value) => ({
      ...value,
      revision: value.revision + 1,
    }));
  });

  it("distinguishes provider groups from different agents with the same display name", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Shared fixture",
        appType: "codex",
        taskEligible: true,
        classifierEligible: false,
        models: [{ id: "fixture-model", name: "Shared model" }],
      },
      {
        id: "claude-provider",
        name: "Shared fixture",
        appType: "claude",
        taskEligible: true,
        classifierEligible: true,
        models: [{ id: "fixture-model", name: "Shared model" }],
      },
    ]);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    const sections = screen.getAllByRole("group", { name: "Shared fixture" });
    expect(sections[0]).toHaveTextContent("codex");
    expect(sections[1]).toHaveTextContent("claude");
    expect(sections[0]).toHaveTextContent("· provider");
    expect(sections[1]).toHaveTextContent("· claude-provider");
    fireEvent.click(
      within(sections[0]).getByRole("checkbox", { name: "Shared model" }),
    );
    fireEvent.click(
      within(sections[1]).getByRole("checkbox", { name: "Shared model" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          members: [expect.objectContaining({ providerId: "claude-provider" })],
        }),
      ),
    );
  });
  it("requires explicit classifier consent and ignores late trials after edits", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          classifierTarget: { providerId: "provider", modelId: "small-model" },
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Classifier fixture",
        taskEligible: false,
        classifierEligible: true,
      },
    ]);
    let finish!: (value: any) => void;
    vi.mocked(intentRoutingApi.trial).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.change(screen.getByLabelText("intentRouting.trialText"), {
      target: { value: "Synthetic debugging task" },
    });
    const run = screen.getByRole("button", { name: "intentRouting.runTrial" });
    expect(run).toBeDisabled();
    fireEvent.click(
      screen.getByRole("checkbox", { name: "intentRouting.trialConsent" }),
    );
    fireEvent.click(run);
    expect(intentRoutingApi.trial).toHaveBeenCalledWith(
      expect.objectContaining({ id: "fixture" }),
      "Synthetic debugging task",
      true,
    );
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "New draft" },
    });
    expect(
      screen.getByRole("checkbox", { name: "intentRouting.trialConsent" }),
    ).not.toBeChecked();
    finish({
      plan: { candidates: group.members },
      intentId: "debug",
      outcome: "matched",
    });
    await waitFor(() =>
      expect(
        screen.queryByText("intentRouting.trialOutcome.matched"),
      ).not.toBeInTheDocument(),
    );
  });

  it("shows the classifier deadline and updates the timeout guidance at the limit", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          classifierTarget: { providerId: "provider", modelId: "small-model" },
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.trial).mockResolvedValue({
      plan: {
        groupId: group.id,
        revision: group.revision,
        candidates: group.members,
        reason: "default_target",
        live: false,
      },
      intentId: null,
      outcome: "timeout",
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.click(screen.getByText("intentRouting.trialTitle"));
    fireEvent.change(screen.getByLabelText("intentRouting.trialText"), {
      target: { value: "Synthetic task" },
    });
    const consent = screen.getByRole("checkbox", {
      name: "intentRouting.trialConsent",
    });
    const run = screen.getByRole("button", { name: "intentRouting.runTrial" });
    fireEvent.click(consent);
    fireEvent.click(run);
    const timeout = "intentRouting.trialOutcome.timeout 2000";
    expect(await screen.findByText(timeout)).toBeInTheDocument();

    fireEvent.click(screen.getByText("routingPage.classifierAdvanced"));
    const deadline = screen.getByLabelText("intentRouting.budget");
    fireEvent.change(deadline, { target: { value: "" } });
    expect(deadline).toHaveValue(null);
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
    fireEvent.click(consent);
    expect(run).toBeDisabled();
    fireEvent.change(deadline, {
      target: { value: "5000" },
    });
    expect(consent).not.toBeChecked();
    expect(screen.queryByText(timeout)).not.toBeInTheDocument();
    fireEvent.click(consent);
    fireEvent.click(run);
    expect(intentRoutingApi.trial).toHaveBeenLastCalledWith(
      expect.objectContaining({ classificationBudgetMs: 5000 }),
      "Synthetic task",
      true,
    );
    expect(
      await screen.findByText("intentRouting.trialOutcome.timeout 5000"),
    ).toBeInTheDocument();

    fireEvent.change(deadline, { target: { value: "15001" } });
    expect(deadline).toHaveAttribute("aria-invalid", "true");
    expect(deadline).toHaveAccessibleDescription("intentRouting.budgetRange");
    expect(run).toBeDisabled();
    expect(deadline).toHaveAttribute("max", "15000");
    fireEvent.change(deadline, { target: { value: "15000" } });
    expect(screen.queryByText("intentRouting.budgetRange")).toBeNull();
    fireEvent.click(consent);
    fireEvent.click(run);
    expect(intentRoutingApi.trial).toHaveBeenLastCalledWith(
      expect.objectContaining({ classificationBudgetMs: 15000 }),
      "Synthetic task",
      true,
    );
    expect(
      await screen.findByText(
        "intentRouting.trialOutcome.timeoutAtLimit 15000",
      ),
    ).toBeInTheDocument();
  });

  it("loads global resources without automatically publishing legacy groups", async () => {
    mount();
    expect(await screen.findByText("Fixture group")).toBeInTheDocument();
    expect(
      screen.getByLabelText("intentRouting.globalResource"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("img", { name: "Codex" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("intentRouting.gatewayAccessHelp"),
    ).not.toBeInTheDocument();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("switch", { name: "intentRouting.autoCreate" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Updated" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "Updated",
          enabled: false,
          revision: 1,
        }),
      ),
    );
  });

  it("requires an explicit publication edit and save for gateway access", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    mount(client);
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    const publish = screen.getByRole("switch", {
      name: "intentRouting.publishGateway",
    });
    expect(publish).not.toBeChecked();
    expect(
      screen.getByText("intentRouting.publishGatewayHelp"),
    ).toBeInTheDocument();
    fireEvent.click(publish);
    expect(publish).toBeChecked();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          id: group.id,
          client: "codex",
          enabled: true,
        }),
      ),
    );
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: ["gateway-catalog"],
      }),
    );
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["agent-model-catalog"],
    });
  });

  it("preserves explicit publication state when editing a global group", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [{ ...group, client: "global", enabled: true }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    expect(
      screen.getByRole("switch", { name: "intentRouting.publishGateway" }),
    ).toBeChecked();
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Renamed" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          client: "global",
          enabled: true,
          name: "Renamed",
        }),
      ),
    );
  });

  it("creates an enabled global group from exact catalog model references", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
        models: [
          { id: "exact-model", name: "Exact model" },
          { id: "second-model", name: "Second model" },
        ],
      },
    ]);
    mount();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "intentRouting.create" }),
      ).toBeEnabled(),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.create" }),
    );
    chooseCustom();
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "New group" },
    });
    expect(screen.getByRole("checkbox", { name: "Exact model" })).toBeChecked();
    fireEvent.click(screen.getByRole("checkbox", { name: "Second model" }));
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          enabled: true,
          revision: 0,
          client: "global",
          members: [
            expect.objectContaining({
              providerId: "provider",
              modelId: "exact-model",
            }),
          ],
        }),
      ),
    );
  });

  it("uses shared selects in advanced routing settings and can clear the classifier", async () => {
    const user = userEvent.setup();
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: false,
      },
      {
        id: "none",
        name: "Classifier fixture",
        taskEligible: false,
        classifierEligible: true,
      },
    ]);
    mount();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    await user.click(screen.getByText("routingPage.classifierAdvanced"));
    for (const name of ["defaultTarget", "classifierProvider"]) {
      const trigger = screen.getByRole("combobox", {
        name: `intentRouting.${name}`,
      });
      expect(trigger.tagName).toBe("BUTTON");
      expect(trigger).toHaveClass("rounded-sm");
    }
    const classifier = screen.getByRole("combobox", {
      name: "intentRouting.classifierProvider",
    });
    await user.click(classifier);
    await user.click(
      await screen.findByRole("option", { name: "Classifier fixture" }),
    );
    expect(
      screen.getByLabelText("intentRouting.classifierModel"),
    ).toBeInTheDocument();
    await user.click(classifier);
    await user.click(
      await screen.findByRole("option", { name: "intentRouting.none" }),
    );
    expect(
      screen.queryByLabelText("intentRouting.classifierModel"),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({ classifierTarget: null }),
      ),
    );
  });

  it("invalidates offline plan results on edits and ignores stale completions", async () => {
    let resolve!: (value: any) => void;
    vi.mocked(intentRoutingApi.preview).mockReturnValue(
      new Promise((r) => {
        resolve = r;
      }),
    );
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.preview" }),
    );
    expect(intentRoutingApi.preview).toHaveBeenCalledWith(
      expect.objectContaining({ id: "fixture" }),
      '{"intentId":null}',
    );
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Changed" },
    });
    resolve({
      groupId: "fixture",
      revision: 1,
      reason: "default_target",
      live: false,
      candidates: [
        { id: "stale", providerId: "provider", modelId: "stale-model" },
      ],
    });
    await waitFor(() =>
      expect(screen.queryByText("stale-model")).not.toBeInTheDocument(),
    );
    expect(screen.getByText("intentRouting.offlineNotice")).toBeInTheDocument();
  });

  it("refreshes generated groups without changing the saved discovery preference", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group],
      autoCreate: true,
      omittedGroups: 0,
    });
    mount();
    await screen.findByText("Fixture group");
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        group,
        {
          ...group,
          id: "auto-fixture",
          name: "shared-model",
          revision: 0,
          autoGenerated: true,
        },
      ],
      autoCreate: true,
      omittedGroups: 0,
    });
    fireEvent.click(
      screen.getByRole("button", { name: "routingPage.refresh" }),
    );
    expect(await screen.findByText("shared-model")).toBeInTheDocument();
    expect(screen.getByText("intentRouting.autoGenerated")).toBeInTheDocument();
    expect(intentRoutingApi.setAutoCreate).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("switch", { name: "intentRouting.autoCreate" }),
    ).not.toBeInTheDocument();
  });

  it("keeps account members editable while showing their transport limitation", async () => {
    const user = userEvent.setup();
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Official account fixture",
        taskEligible: false,
        classifierEligible: false,
      },
    ]);
    mount();
    await screen.findByText("Fixture group");
    expect(
      await screen.findByTitle("intentRouting.noMemberReady"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "common.edit" }));
    const provider = screen.getByRole("group", {
      name: "Official account fixture",
    });
    expect(
      within(provider).getByRole("checkbox", { name: "fixture-model" }),
    ).toBeChecked();
    expect(
      within(provider).getByText(
        "intentRouting.memberUnavailable.providerNotReady",
      ),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          enabled: false,
          members: group.members,
        }),
      ),
    );
  });

  it("does not enable discovery when entering and leaving batch management", async () => {
    mount();
    await openManagement();
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await openManagement();
    expect(intentRoutingApi.setAutoCreate).not.toHaveBeenCalled();
    expect(intentRoutingApi.refreshCatalogs).not.toHaveBeenCalled();
    expect(intentRoutingApi.discoverCatalogs).not.toHaveBeenCalled();
    expect(
      screen.queryByText("intentRouting.autoCreate"),
    ).not.toBeInTheDocument();
  });

  it("requires confirmation to delete and never displays raw backend errors", async () => {
    vi.mocked(intentRoutingApi.remove).mockRejectedValue(
      "synthetic-private-error",
    );
    mount();
    fireEvent.click(
      await screen.findByRole("button", { name: "common.delete" }),
    );
    expect(intentRoutingApi.remove).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.confirmDelete" }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.remove).toHaveBeenCalledWith("fixture", 1),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    expect(
      screen.queryByText("synthetic-private-error"),
    ).not.toBeInTheDocument();
  });
});
