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
import userEvent from "@testing-library/user-event";
import { copyText } from "@/lib/clipboard";
import { toast } from "sonner";
import { routingTemplates } from "@/lib/routingGroupTransfer";
vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn() }));
vi.mock("sonner", () => ({ toast: { success: vi.fn() } }));

vi.mock("@/components/routing/RoutingClient", () => ({
  RoutingClient: () => null,
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
    t: (key: string) => key,
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
) {
  return render(
    <QueryClientProvider client={client}>
      <div role="tabpanel" tabIndex={0}>
        <IntentRoutingPanel maskAccounts={maskAccounts} />
      </div>
    </QueryClientProvider>,
  );
}

async function openManagement() {
  const manage = await screen.findByRole("button", {
    name: "routingPage.manage",
  });
  await waitFor(() => expect(manage).toBeEnabled());
  fireEvent.click(manage);
}

describe("IntentRoutingPanel", () => {
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
      expect(within(row(second.id)).getByRole("combobox")).toBeEnabled();
      expect(
        within(row(second.id)).getByRole("button", { name: "common.edit" }),
      ).toBeEnabled();
      for (const name of [
        "routingPage.import",
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

  it("keeps other rows and page controls usable while a strategy save is pending", async () => {
    const second = { ...group, id: "second-group", name: "Second group" };
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, second],
      autoCreate: false,
      omittedGroups: 0,
    });
    let finish!: (value: RoutingGroup) => void;
    vi.mocked(intentRoutingApi.save).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const { container } = mount();
    await screen.findByText("Second group");
    const firstRow = container.querySelector<HTMLElement>(
      '[data-routing-group="fixture"]',
    )!;
    const secondRow = container.querySelector<HTMLElement>(
      '[data-routing-group="second-group"]',
    )!;
    const user = userEvent.setup();
    await user.click(within(firstRow).getByRole("combobox"));
    await user.click(
      screen.getByRole("option", { name: "intentRouting.routingSmart" }),
    );
    expect(within(firstRow).getByRole("switch")).toBeDisabled();
    expect(within(secondRow).getByRole("switch")).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "routingPage.import" }),
    ).toBeEnabled();
    await act(async () => finish({ ...group, routing: "smart", revision: 2 }));
    await waitFor(() =>
      expect(within(firstRow).getByRole("switch")).toBeEnabled(),
    );
  });

  it("keeps a copy operation scoped to its source row", async () => {
    const second = { ...group, id: "second-group", name: "Second group" };
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, second],
      autoCreate: false,
      omittedGroups: 0,
    });
    let finish!: (value: RoutingGroup) => void;
    vi.mocked(intentRoutingApi.save).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const { container } = mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", {
        name: "routingPage.more Fixture group",
      }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "routingPage.duplicate" }),
    );
    const firstRow = container.querySelector<HTMLElement>(
      '[data-routing-group="fixture"]',
    )!;
    const secondRow = container.querySelector<HTMLElement>(
      '[data-routing-group="second-group"]',
    )!;
    expect(firstRow).toHaveAttribute("aria-busy", "true");
    expect(within(secondRow).getByRole("switch")).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "routingPage.import" }),
    ).toBeEnabled();
    const duplicate = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    await act(async () => finish({ ...duplicate, revision: 1 }));
    await waitFor(() => expect(firstRow).toHaveAttribute("aria-busy", "false"));
  });

  it.each([false, true])(
    "queues concurrent copies without losing groups (first fails: %s)",
    async (firstFails) => {
      const second = { ...group, id: "second-group", name: "Second group" };
      let catalog: RoutingGroup[] = [group, second];
      const finishes: Array<() => void> = [];
      vi.mocked(intentRoutingApi.list).mockImplementation(async () => ({
        groups: catalog,
        autoCreate: false,
        omittedGroups: 0,
      }));
      vi.mocked(intentRoutingApi.save).mockImplementation(
        (value) =>
          new Promise((resolve, reject) => {
            const first = finishes.length === 0;
            finishes.push(() => {
              if (first && firstFails) reject("intentRouting.conflict");
              else {
                const saved = { ...value, revision: 1 };
                catalog = [...catalog, saved];
                resolve(saved);
              }
            });
          }),
      );
      vi.mocked(intentRoutingApi.arrange).mockImplementation(async (ids) => {
        expect(new Set(ids)).toEqual(new Set(catalog.map((entry) => entry.id)));
        catalog = ids.map((id) => catalog.find((entry) => entry.id === id)!);
      });
      const { container } = mount();
      const user = userEvent.setup();
      for (const name of [group.name, second.name]) {
        await user.click(
          await screen.findByRole("button", {
            name: `routingPage.more ${name}`,
          }),
        );
        await user.click(
          screen.getByRole("menuitem", { name: "routingPage.duplicate" }),
        );
      }
      const secondRow = container.querySelector<HTMLElement>(
        '[data-routing-group="second-group"]',
      )!;
      expect(secondRow).toHaveAttribute("aria-busy", "true");
      expect(intentRoutingApi.save).toHaveBeenCalledTimes(1);
      await act(async () => finishes[0]());
      await waitFor(() =>
        expect(intentRoutingApi.save).toHaveBeenCalledTimes(2),
      );
      expect(within(secondRow).getByRole("switch")).toBeDisabled();
      await act(async () => finishes[1]());
      await waitFor(() =>
        expect(within(secondRow).getByRole("switch")).toBeEnabled(),
      );
      const copies = vi
        .mocked(intentRoutingApi.save)
        .mock.calls.map(([value]) => value);
      expect(catalog.map((entry) => entry.id)).toEqual(
        firstFails
          ? [group.id, second.id, copies[1].id]
          : [group.id, copies[0].id, second.id, copies[1].id],
      );
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

  it("keeps template cards visible while Manage only changes batch selection", async () => {
    mount();
    await screen.findByText("Fixture group");
    const table = screen.getByRole("table", { name: "intentRouting.title" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent),
    ).toEqual([
      "routingPage.groupName",
      "routingPage.upstreamCount",
      "routingPage.requestCount",
      "routingPage.switchCount",
      "routingPage.agentErrorCount",
      "routingPage.actions",
    ]);
    expect(
      screen.queryByRole("switch", { name: "intentRouting.autoCreate" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "routingPage.manage" }));
    expect(
      screen.queryByRole("switch", { name: "intentRouting.autoCreate" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("routingPage.templateSmart")).toBeInTheDocument();
    expect(
      screen.getByRole("region", { name: "routingPage.templates" }),
    ).toBeVisible();
    expect(
      screen.getByRole("checkbox", { name: "Fixture group" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "routingPage.removeSelected" }),
    ).toBeDisabled();
    const row = within(table).getAllByRole("row")[1];
    expect(
      within(row).getAllByTitle("routingPage.groupStatsUnavailable"),
    ).toHaveLength(3);
  });

  it("keeps strategy, publication and editing together in Actions without showing IDs", async () => {
    mount();
    const heading = await screen.findByRole("heading", {
      name: "Fixture group",
    });
    const row = heading.closest("tr")!;
    const cells = within(row).getAllByRole("cell");
    expect(cells).toHaveLength(6);
    expect(
      within(row).queryByText("vary-auto/fixture"),
    ).not.toBeInTheDocument();
    expect(within(cells[0]).queryByRole("combobox")).not.toBeInTheDocument();
    expect(within(cells[0]).queryByRole("switch")).not.toBeInTheDocument();
    expect(
      within(cells[5]).getByRole("combobox", {
        name: "intentRouting.routingStrategy",
      }),
    ).toBeInTheDocument();
    const publication = within(cells[5]).getByRole("switch", {
      name: "intentRouting.publishGateway Fixture group",
    });
    expect(publication).not.toBeChecked();
    expect(publication).toHaveAttribute("data-state", "unchecked");
    expect(publication).toHaveTextContent(/^$/);
    const edit = within(cells[5]).getByRole("button", { name: "common.edit" });
    expect(edit).toHaveTextContent(/^$/);
    expect(edit).toHaveAttribute("title", "common.edit");
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
    fireEvent.change(status, { target: { value: "enabled" } });
    expect(screen.queryByText("Fixture group")).not.toBeInTheDocument();
    expect(screen.getByText("Enabled group")).toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("searchbox", { name: "intentRouting.filter" }),
      { target: { value: "missing" } },
    );
    expect(screen.getByText("intentRouting.noMatches")).toBeInTheDocument();
    expect(screen.queryByTestId("routing-group-count")).not.toBeInTheDocument();
    fireEvent.change(status, { target: { value: "disabled" } });
    fireEvent.change(
      screen.getByRole("searchbox", { name: "intentRouting.filter" }),
      { target: { value: "fixture-model" } },
    );
    expect(screen.getByText("Fixture group")).toBeInTheDocument();
    expect(screen.queryByText("Enabled group")).not.toBeInTheDocument();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("counts distinct configured and matched upstreams in each table row", async () => {
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
    expect(within(row).getAllByRole("cell")[1]).toHaveTextContent("2");
  });

  it("places routing directly below the name and keeps state-specific rate-limit help", async () => {
    mount();
    await screen.findByText("Fixture group");
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.create" }),
    );
    const form = screen.getByRole("form");
    const routing = within(form).getByRole("radiogroup", {
      name: "intentRouting.routingStrategy",
    });
    expect(within(routing).getAllByRole("radio")).toHaveLength(9);
    const fields = [...form.querySelectorAll("[data-routing-field]")].map(
      (node) => node.getAttribute("data-routing-field"),
    );
    expect(fields.slice(0, 5)).toEqual([
      "name",
      "routing",
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
    fireEvent.click(
      within(routing).getByRole("radio", {
        name: "intentRouting.routingRoundRobin",
      }),
    );
    expect(
      within(form).queryByRole("radiogroup", { name: "intentRouting.sink" }),
    ).toBeNull();
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

  it("clears an inapplicable sink setting only when the edited group is saved", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [{ ...group, sink: true }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    const routing = screen.getByRole("radiogroup", {
      name: "intentRouting.routingStrategy",
    });
    fireEvent.click(
      within(routing).getByRole("radio", {
        name: "intentRouting.routingWeighted",
      }),
    );
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({ routing: "weighted", sink: false }),
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
    expect(
      summary?.querySelector("[title]")?.getAttribute("title"),
    ).not.toContain("fixture.account@example.test");
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
      expect(screen.getByText("routingPage.templateSmart")).toBeInTheDocument();
      expect(
        screen.getByRole("checkbox", { name: "Fixture group" }),
      ).toBeInTheDocument();
      expect(intentRoutingApi.setAutoCreate).not.toHaveBeenCalled();
      expect(intentRoutingApi.setAutoGroupSkip).not.toHaveBeenCalled();
    },
  );

  it("restores a removed discovered group without saving a manual definition", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group],
      autoCreate: true,
      omittedGroups: 0,
      removedGroups: [{ id: "auto-fixture", name: "Synthetic shared" }],
    });
    vi.mocked(intentRoutingApi.restore).mockResolvedValue();
    mount();
    await userEvent.click(
      await screen.findByRole("button", {
        name: "intentRouting.removedGroups",
      }),
    );
    fireEvent.click(
      await screen.findByRole("menuitem", { name: /Synthetic shared/ }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.restore).toHaveBeenCalledWith("auto-fixture"),
    );
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });
  it("removes an untouched automatic group through confirmation instead of saving a disabled override", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          id: "auto-fixture",
          revision: 0,
          enabled: true,
          autoGenerated: true,
        },
      ],
      autoCreate: true,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.remove).mockResolvedValue();
    mount();
    await userEvent.click(
      await screen.findByRole("switch", {
        name: "intentRouting.publishGateway Fixture group",
      }),
    );
    expect(intentRoutingApi.remove).not.toHaveBeenCalled();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole("button", { name: "intentRouting.confirmDelete" }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.remove).toHaveBeenCalledWith("auto-fixture", 0),
    );
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });
  it("keeps removed groups recoverable and redacts backend restoration failures", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group],
      autoCreate: true,
      omittedGroups: 0,
      removedGroups: [{ id: "auto-fixture", name: "Synthetic shared" }],
    });
    vi.mocked(intentRoutingApi.restore).mockRejectedValue(
      "synthetic-private-failure",
    );
    mount();
    await userEvent.click(
      await screen.findByRole("button", {
        name: "intentRouting.removedGroups",
      }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: /Synthetic shared/ }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    expect(
      screen.queryByText("synthetic-private-failure"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "intentRouting.removedGroups" }),
    ).toBeEnabled();
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
          manualMemberId: "member",
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
    expect(saved.manualMemberId).toBe("next");
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
  it("shows why templates are unavailable instead of offering generic replacements", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    expect(
      await screen.findByText("routingPage.templateSmart"),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText("routingPage.templateNeed_provider"),
    ).toHaveLength(3);
    const actions = screen.getAllByRole("button", {
      name: "routingPage.addTemplate",
    });
    expect(actions).toHaveLength(3);
    for (const action of actions) expect(action).toBeDisabled();
    expect(
      screen.queryByText("routingPage.templateFallback"),
    ).not.toBeInTheDocument();
  });
  it.each(["templateSmart", "templateThrifty", "templateSteady"] as const)(
    "opens %s from its always-visible card with fields ready for review",
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
      const expected = routingTemplates(providers).find(
        (template) => template.key === key,
      )!.group;
      vi.mocked(intentRoutingApi.providers).mockResolvedValue(providers);
      mount();
      const user = userEvent.setup();
      const open = async () => {
        const card = (await screen.findByText(`routingPage.${key}`)).closest(
          "div",
        )!;
        expect(
          screen.getAllByRole("button", { name: "routingPage.addTemplate" }),
        ).toHaveLength(3);
        await user.click(
          within(card).getByRole("button", { name: "routingPage.addTemplate" }),
        );
        expect(
          screen.queryByRole("dialog", { name: "intentRouting.discardTitle" }),
        ).not.toBeInTheDocument();
        const dialog = screen.getByRole("dialog", {
          name: `routingPage.${key}`,
        });
        expect(dialog).toContainElement(screen.getByRole("form"));
        expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
          `routingPage.${key}`,
        );
        expect(
          dialog.querySelectorAll(".rt-model-chip input:checked"),
        ).toHaveLength(
          new Set(
            expected.members.map((member) =>
              JSON.stringify([member.providerId, member.modelId]),
            ),
          ).size,
        );
        expect(
          screen.getByRole("radio", { name: "intentRouting.routingOrdered" }),
        ).toHaveAttribute("aria-checked", "true");
        expect(
          screen.getByRole("button", { name: "common.save" }),
        ).toBeEnabled();
        expect(intentRoutingApi.save).not.toHaveBeenCalled();
      };
      await open();
      await user.click(screen.getByRole("button", { name: "common.cancel" }));
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
      await open();
      fireEvent.change(screen.getByLabelText("intentRouting.name"), {
        target: { value: "Reviewed template" },
      });
      await user.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalledOnce());
      const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
      expect(saved).toMatchObject({
        name: "Reviewed template",
        routing: "ordered",
        classifierTarget: expected.classifierTarget,
        intentRecognitionEnabled: expected.intentRecognitionEnabled,
      });
      expect(
        saved.members.map(({ providerId, modelId }) => [providerId, modelId]),
      ).toEqual(
        expected.members.map(({ providerId, modelId }) => [
          providerId,
          modelId,
        ]),
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
  it("duplicates an offline group with fresh references and keeps local rules without requiring discovery", async () => {
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", {
        name: "routingPage.more Fixture group",
      }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: /routingPage.duplicate/ }),
    );
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalled());
    const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    expect(saved.id).not.toBe(group.id);
    expect(saved.members[0].id).not.toBe(group.members[0].id);
    expect(saved.enabled).toBe(true);
    expect(saved.rules[0].targetMemberId).toBe(saved.members[0].id);
    await waitFor(() =>
      expect(intentRoutingApi.arrange).toHaveBeenCalledWith([
        group.id,
        saved.id,
      ]),
    );
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
  });
  it("reports clipboard failures without rendering native diagnostics or opening an editor", async () => {
    vi.mocked(copyText).mockRejectedValue(
      new Error("synthetic-private-clipboard-error"),
    );
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [{ ...group, rules: [] }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", {
        name: "routingPage.more Fixture group",
      }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: /routingPage.copyShare/ }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    expect(
      screen.queryByText(/synthetic-private-clipboard-error/),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(JSON.parse(vi.mocked(copyText).mock.calls[0][0])).toMatchObject({
      magpie: "routing-group",
      version: 1,
    });
    expect(toast.success).not.toHaveBeenCalled();
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
  it("shows Duplicate, Copy to share and JSON export in the Actions More menu", async () => {
    mount();
    const user = userEvent.setup();
    const heading = await screen.findByRole("heading", {
      name: "Fixture group",
    });
    const row = heading.closest("tr")!;
    fireEvent.contextMenu(heading);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    await user.click(
      within(row).getByRole("button", {
        name: "routingPage.arrangeGroup Fixture group",
      }),
    );
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    const actions = within(row).getAllByRole("cell").at(-1)!;
    const more = within(actions).getByRole("button", {
      name: "routingPage.more Fixture group",
    });
    expect(within(row).getAllByRole("cell")[0]).not.toContainElement(more);
    await user.click(more);
    expect(
      screen.getAllByRole("menuitem").map((item) => item.textContent),
    ).toEqual([
      "routingPage.duplicate",
      "routingPage.copyShare",
      "routingPage.export",
    ]);
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    await user.click(
      screen.getByRole("button", { name: "routingPage.manage" }),
    );
    expect(more).toBeDisabled();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
    expect(intentRoutingApi.arrange).not.toHaveBeenCalled();
  });

  it("retains keyboard group ordering without menu entries", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, { ...group, id: "next", name: "Next group" }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    fireEvent.keyDown(
      await screen.findByRole("button", {
        name: "routingPage.arrangeGroup Fixture group",
      }),
      { key: "ArrowDown", altKey: true },
    );
    await waitFor(() =>
      expect(intentRoutingApi.arrange).toHaveBeenCalledWith([
        "next",
        "fixture",
      ]),
    );
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("shows a success toast only after copying without inserting a status into the row", async () => {
    let finish!: () => void;
    vi.mocked(copyText).mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [{ ...group, rules: [] }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", {
        name: "routingPage.more Fixture group",
      }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "routingPage.copyShare" }),
    );
    expect(copyText).toHaveBeenCalledOnce();
    expect(toast.success).not.toHaveBeenCalled();
    await act(async () => {
      finish();
    });
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith("routingPage.copied"),
    );
    expect(screen.queryByText("routingPage.copied")).not.toBeInTheDocument();
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(JSON.parse(vi.mocked(copyText).mock.calls[0][0])).toMatchObject({
      magpie: "routing-group",
      version: 1,
    });
  });

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
  it("saves a rule's selected fallback seats in their click order", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          members: [
            group.members[0],
            { ...group.members[0], id: "variant", effort: "none" },
          ],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    await user.click(
      screen.getByRole("button", { name: "routingPage.addRuleModel" }),
    );
    await user.click(
      screen.getByRole("menuitem", { name: "fixture-model :none" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(intentRoutingApi.save).toHaveBeenCalled());
    const saved = vi.mocked(intentRoutingApi.save).mock.calls[0][0];
    expect(saved.rules[0].fallbackMemberIds).toEqual([saved.members[1].id]);
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
  it.each(["cancel", "escape"] as const)(
    "opens import in a focused dialog and resets it after %s",
    async (dismiss) => {
      const user = userEvent.setup();
      mount();
      await screen.findByText("Fixture group");
      const table = screen.getByRole("table", { name: "intentRouting.title" });
      const trigger = screen.getByRole("button", {
        name: "routingPage.import",
      });
      await user.click(trigger);
      const dialog = screen.getByRole("dialog", { name: "routingPage.import" });
      const input = within(dialog).getByRole("textbox", {
        name: "routingPage.importLabel",
      });
      expect(input).toHaveFocus();
      expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
      expect(table.closest(".rt-group-panel")).not.toContainElement(dialog);
      fireEvent.change(input, { target: { value: "synthetic import draft" } });
      if (dismiss === "cancel")
        await user.click(
          within(dialog).getByRole("button", { name: "common.cancel" }),
        );
      else await user.keyboard("{Escape}");
      await waitFor(() =>
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
      );
      expect(trigger).toHaveFocus();
      await user.click(trigger);
      expect(
        screen.getByRole("textbox", { name: "routingPage.importLabel" }),
      ).toHaveValue("");
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
    },
  );

  it("keeps the import dialog open during saving and returns focus after completion", async () => {
    let finishSave!: () => void;
    vi.mocked(intentRoutingApi.save).mockImplementation(
      (value) =>
        new Promise((resolve) => {
          finishSave = () =>
            resolve({ ...value, revision: value.revision + 1 });
        }),
    );
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: true,
        models: [{ id: "fixture-model", name: "Fixture model" }],
      },
    ]);
    const user = userEvent.setup();
    mount();
    await screen.findByText("Fixture group");
    const trigger = screen.getByRole("button", { name: "routingPage.import" });
    await user.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "routingPage.import" });
    const input = within(dialog).getByRole("textbox", {
      name: "routingPage.importLabel",
    });
    fireEvent.change(input, {
      target: {
        value: JSON.stringify({
          name: "Imported fixture",
          members: ["provider/fixture-model"],
        }),
      },
    });
    await user.click(
      within(dialog).getByRole("button", { name: "routingPage.reviewImport" }),
    );
    expect(dialog).toHaveAttribute("aria-busy", "true");
    expect(input).toBeDisabled();
    expect(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog", { name: "routingPage.import" })).toBe(
      dialog,
    );
    finishSave();
    await screen.findByRole("status", { name: "routingPage.importComplete" });
    const done = screen.getByRole("button", { name: "common.done" });
    await waitFor(() => expect(done).toBeEnabled());
    await user.click(done);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(trigger).toHaveFocus();
    expect(intentRoutingApi.save).toHaveBeenCalledOnce();
  });

  it("imports immediately and displays a completion result without opening the editor", async () => {
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: true,
        models: [{ id: "fixture-model", name: "Fixture model" }],
      },
    ]);
    mount();
    fireEvent.click(
      await screen.findByRole("button", { name: "routingPage.import" }),
    );
    fireEvent.change(screen.getByLabelText("routingPage.importLabel"), {
      target: {
        value: JSON.stringify({
          name: "Imported fixture",
          members: ["provider/fixture-model"],
          routing: "order",
        }),
      },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "routingPage.reviewImport" }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          client: "global",
          routing: "ordered",
          name: "Imported fixture",
        }),
      ),
    );
    expect(
      await screen.findByRole("status", { name: "routingPage.importComplete" }),
    ).toHaveTextContent("routingPage.imported");
    expect(
      screen.queryByRole("textbox", { name: "intentRouting.name" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.done" }));
    expect(
      screen.queryByLabelText("routingPage.importComplete"),
    ).not.toBeInTheDocument();
  });

  it("protects pasted import text and keeps it after a failed save without revealing errors", async () => {
    vi.mocked(intentRoutingApi.save).mockRejectedValue(
      "synthetic-private-error",
    );
    vi.mocked(intentRoutingApi.providers).mockResolvedValue([
      {
        id: "provider",
        name: "Fixture provider",
        taskEligible: true,
        classifierEligible: true,
        models: [{ id: "fixture-model", name: "Fixture model" }],
      },
    ]);
    const editing = vi.fn();
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <IntentRoutingPanel onEditingChange={editing} />
      </QueryClientProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "routingPage.import" }),
    );
    fireEvent.change(screen.getByLabelText("routingPage.importLabel"), {
      target: {
        value: JSON.stringify({
          name: "Imported fixture",
          members: ["provider/fixture-model"],
        }),
      },
    });
    await waitFor(() =>
      expect(editing).toHaveBeenLastCalledWith({ dirty: true, busy: false }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "routingPage.reviewImport" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    expect(
      (screen.getByLabelText("routingPage.importLabel") as HTMLTextAreaElement)
        .value,
    ).toContain("Imported fixture");
    expect(
      screen.queryByText("synthetic-private-error"),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await waitFor(() =>
      expect(editing).toHaveBeenLastCalledWith({ dirty: false, busy: false }),
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
          screen.getByText("routingPage.templateSmart"),
        ).toBeInTheDocument();
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
      screen.getByRole("dialog", { name: "routingPage.removeSelected" }),
    ).toBe(activeConfirmation);
    expect(confirm).toBeDisabled();
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
    expect(document.querySelectorAll(".rt-model-chips")).not.toHaveLength(0);
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
          members: [{ ...group.members[0], effort: "high", weight: 7 }],
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
            { ...group.members[0], effort: "high", weight: 7 },
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
    expect(screen.getByText("routingPage.templateHint")).toBeInTheDocument();
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
        models: [{ id: "one", name: "One" }],
      },
    ]);
    mount();
    await screen.findByText("Fixture group");
    const list = screen.getByRole("rowgroup", { name: "intentRouting.title" });
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.create" }),
    );
    const dialog = screen.getByRole("dialog", { name: "intentRouting.create" });
    const editor = screen.getByRole("form", { name: "intentRouting.create" });
    expect(dialog).toContainElement(editor);
    expect(list).not.toContainElement(editor);
    expect(list.children).toHaveLength(1);
    expect(screen.getByLabelText("intentRouting.name")).toHaveFocus();
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
      mount();
      await screen.findByText("Fixture group");
      const trigger = screen.getByRole("button", {
        name: "intentRouting.create",
      });
      await user.click(trigger);
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
      expect(screen.getByLabelText("intentRouting.name")).toHaveValue("");
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
    expect(
      await screen.findByRole("dialog", { name: "intentRouting.create" }),
    ).toContainElement(screen.getByLabelText("intentRouting.name"));
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
    dialog = screen.getByRole("dialog", { name: "intentRouting.discardTitle" });
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "intentRouting.discardChanges",
      }),
    );
    expect(
      screen.getByRole("form", { name: "intentRouting.create" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue("");
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

  it("offers all nine strategies and saves a manual member from the compact row", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          members: [
            ...group.members,
            { id: "second", providerId: "provider", modelId: "other-model" },
          ],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("combobox", {
        name: "intentRouting.routingStrategy",
      }),
    );
    expect(screen.getAllByRole("option")).toHaveLength(9);
    await user.click(
      screen.getByRole("option", { name: "intentRouting.routingManual" }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          routing: "manual",
          manualMemberId: "member",
        }),
      ),
    );
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
  });

  it("preserves saved member weights and free status when changing strategy", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        { ...group, members: [{ ...group.members[0], weight: 3, free: true }] },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    await user.click(
      screen.getByRole("radio", { name: "intentRouting.routingWeighted" }),
    );
    await user.click(
      screen.getByRole("radio", { name: "intentRouting.routingFreeFirst" }),
    );
    await user.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          routing: "free_first",
          members: [expect.objectContaining({ weight: 3, free: true })],
        }),
      ),
    );
  });

  it("selects a manual member directly without opening the editor", async () => {
    const manual = {
      ...group,
      routing: "manual" as const,
      manualMemberId: "member",
      members: [
        ...group.members,
        { id: "second", providerId: "provider", modelId: "other-model" },
      ],
    };
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [manual],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("radio", { name: /other-model/ }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          manualMemberId: "second",
          routing: "manual",
        }),
      ),
    );
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
  });

  it("does not mark a manual group unavailable because an unused member is unavailable", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          routing: "manual",
          manualMemberId: "member",
          members: [
            ...group.members,
            { id: "unused", providerId: "missing", modelId: "unavailable" },
          ],
        },
      ],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    await screen.findByText("Fixture group");
    await screen.findByRole("radiogroup", {
      name: "intentRouting.manualMember",
    });
    expect(
      screen.queryByLabelText("intentRouting.memberNotReady"),
    ).not.toBeInTheDocument();
  });

  it("pauses classifier controls in manual mode and retains the stored rules", async () => {
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    await user.click(
      screen.getByRole("radio", { name: "intentRouting.routingManual" }),
    );
    expect(screen.getByLabelText("intentRouting.description")).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "intentRouting.runTrial" }),
    ).toBeDisabled();
    expect(screen.getByLabelText("intentRouting.description")).toHaveValue(
      "Diagnose bugs",
    );
    expect(screen.getByText("intentRouting.manualPaused")).toBeInTheDocument();
    await user.click(
      screen.getByRole("radio", { name: "intentRouting.routingSmart" }),
    );
    expect(screen.getByLabelText("intentRouting.description")).toBeEnabled();
    expect(screen.getByLabelText("intentRouting.description")).toHaveValue(
      "Diagnose bugs",
    );
  });

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
    const otherStrategy = within(list.children[1] as HTMLElement).getByRole(
      "combobox",
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
    expect(otherStrategy).toBeDisabled();
    expect(otherArrange).toBeDisabled();
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Saved group" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(finishRefresh).toBeTypeOf("function"));
    expect(otherStrategy).toBeEnabled();
    expect(otherArrange).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "routingPage.refresh" }),
    ).toBeEnabled();
    expect(
      within(list.children[0] as HTMLElement).getByRole("switch"),
    ).toBeDisabled();
    finishRefresh();
    await screen.findByText("Saved group");
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(list.children).toHaveLength(2);
    expect(
      within(list.children[0] as HTMLElement).getByText("Saved group"),
    ).toBeInTheDocument();
    expect(
      within(list.children[1] as HTMLElement).getByRole("combobox"),
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

  it("hides aliases while keeping alias search and strategy saves intact", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [
        {
          ...group,
          modelAlias: "auto/fixture-model",
          routing: "smart",
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
    await user.click(
      screen.getByRole("combobox", { name: "intentRouting.routingStrategy" }),
    );
    await user.click(
      screen.getByRole("option", { name: "intentRouting.routingRoundRobin" }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "fixture",
          modelAlias: "auto/fixture-model",
          routing: "round_robin",
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

  it.each([true, false])(
    "keeps controls and new drafts available during refresh (empty list: %s)",
    async (empty) => {
      const snapshot = {
        groups: empty ? [] : [structuredClone(group)],
        autoCreate: false,
        omittedGroups: 0,
      };
      vi.mocked(intentRoutingApi.list).mockResolvedValue(snapshot);
      let finishCatalog!: (value: []) => void;
      let finishList!: (value: typeof snapshot) => void;
      vi.mocked(intentRoutingApi.refreshCatalogs).mockReturnValueOnce(
        new Promise((resolve) => {
          finishCatalog = resolve;
        }),
      );
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
        screen.getByRole("button", { name: "routingPage.import" }),
      ).toBeEnabled();
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
      expect(intentRoutingApi.refreshCatalogs).toHaveBeenCalledOnce();
      fireEvent.click(create);
      const name = screen.getByLabelText("intentRouting.name");
      expect(name).toBeEnabled();
      fireEvent.change(name, { target: { value: "Draft during refresh" } });
      await act(async () => finishCatalog([]));
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

  it("ends refresh loading after a safe error and allows retry", async () => {
    vi.mocked(intentRoutingApi.refreshCatalogs)
      .mockRejectedValueOnce("synthetic-private-refresh-error")
      .mockResolvedValue([]);
    mount();
    const reload = await screen.findByRole("button", {
      name: "routingPage.refresh",
    });
    await waitFor(() => expect(reload).toBeEnabled());
    fireEvent.click(reload);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    await waitFor(() => expect(reload).toHaveAttribute("aria-busy", "false"));
    expect(reload).toBeEnabled();
    expect(screen.queryByText("synthetic-private-refresh-error")).toBeNull();
    expect(
      screen.getByRole("button", { name: "routingPage.import" }),
    ).toBeEnabled();
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
      name: "routingPage.refresh",
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
        name: "routingPage.refresh",
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
      screen.getByRole("button", { name: "routingPage.refresh" }),
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
      name: "routingPage.refresh",
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
        models: [{ id: "exact-model", name: "Exact model" }],
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
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "New group" },
    });
    expect(screen.getByRole("checkbox", { name: "Exact model" })).toBeChecked();
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
      expect(trigger).toHaveClass("rounded-full");
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
      await screen.findByTitle("intentRouting.memberNotReady"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "common.edit" }));
    const provider = screen.getByRole("group", {
      name: "Official account fixture",
    });
    expect(
      within(provider).getByRole("checkbox", { name: "fixture-model" }),
    ).toBeChecked();
    expect(
      within(provider).getByText("intentRouting.memberNotReady"),
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
