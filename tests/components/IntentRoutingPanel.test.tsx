import {
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
import { intentRoutingApi } from "@/lib/api/intentRouting";
import userEvent from "@testing-library/user-event";

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
    refreshCatalogs: vi.fn(),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
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

function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: {
            queries: { retry: false },
            mutations: { retry: false },
          },
        })
      }
    >
      <div role="tabpanel" tabIndex={0}>
        <IntentRoutingPanel />
      </div>
    </QueryClientProvider>,
  );
}

describe("IntentRoutingPanel", () => {
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
    const list = await screen.findByRole("list", {
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

  it("expands only the edited row in place and keeps the other groups and toolbar visible", async () => {
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
    const list = await screen.findByRole("list", {
      name: "intentRouting.title",
    });
    fireEvent.click(
      within(list.children[1] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    const editor = screen.getByRole("form", { name: "Fixture group" });
    expect(list.children).toHaveLength(3);
    expect(list.children[1]).toContainElement(editor);
    expect(
      within(list.children[0] as HTMLElement).getByText("Before group"),
    ).toBeInTheDocument();
    expect(
      within(list.children[2] as HTMLElement).getByText("After group"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("searchbox", { name: "intentRouting.filter" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "intentRouting.create" }),
    ).toBeEnabled();
    expect(screen.getByLabelText("intentRouting.name")).toHaveFocus();
    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled();
    fireEvent.click(
      within(editor).getByRole("button", { name: "common.cancel" }),
    );
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(list.children).toHaveLength(3);
    expect(
      within(list.children[1] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    ).toHaveFocus();
    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled();
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("keeps a new group editor above existing rows and preserves it after a failed save", async () => {
    vi.mocked(intentRoutingApi.save).mockRejectedValue(
      "synthetic-private-error",
    );
    mount();
    await screen.findByText("Fixture group");
    fireEvent.click(
      screen.getByRole("button", { name: "intentRouting.create" }),
    );
    const list = screen.getByRole("list", { name: "intentRouting.title" });
    const editor = screen.getByRole("form", { name: "intentRouting.create" });
    expect(list.children).toHaveLength(2);
    expect(list.children[0]).toContainElement(editor);
    expect(
      within(list.children[1] as HTMLElement).getByText("Fixture group"),
    ).toBeInTheDocument();
    fireEvent.click(
      within(editor).getByRole("button", { name: "common.cancel" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Changed draft" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Changed draft",
    );
    expect(
      screen.getByRole("list", { name: "intentRouting.title" }).children,
    ).toHaveLength(1);
    expect(
      screen.queryByText("synthetic-private-error"),
    ).not.toBeInTheDocument();
  });

  it("keeps the active editor mounted when filtering and restores the filter on cancel", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, { ...group, id: "other", name: "Other group" }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const list = await screen.findByRole("list", {
      name: "intentRouting.title",
    });
    fireEvent.click(
      within(list.children[0] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    const editor = screen.getByRole("form", { name: "Fixture group" });
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Unsaved name" },
    });
    const filter = screen.getByRole("searchbox", {
      name: "intentRouting.filter",
    });
    fireEvent.change(filter, { target: { value: "Other group" } });
    expect(screen.getByRole("form", { name: "Fixture group" })).toBe(editor);
    expect(screen.getByText("Other group")).toBeInTheDocument();
    fireEvent.change(filter, { target: { value: "no match" } });
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Unsaved name",
    );
    fireEvent.click(
      within(editor).getByRole("button", { name: "common.cancel" }),
    );
    expect(filter).toHaveValue("no match");
    expect(screen.getByText("intentRouting.noMatches")).toBeInTheDocument();
  });

  it("confirms discarding changed drafts before editing another group or creating one", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, { ...group, id: "other", name: "Other group" }],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    const list = await screen.findByRole("list", {
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
    fireEvent.click(
      within(list.children[1] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    let dialog = screen.getByRole("dialog", {
      name: "intentRouting.discardTitle",
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    );
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Unsaved name",
    );
    fireEvent.click(
      within(list.children[1] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
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
      screen.getByRole("button", { name: "intentRouting.create" }),
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

  it("opens from the row and folds from its title without adding a collapse button", async () => {
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
    const header = within(editor).getByRole("button", {
      name: "Fixture group",
    });
    fireEvent.keyDown(header, { key: "Enter" });
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("heading", { name: "Fixture group" }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Fixture group" }), {
      key: " ",
    });
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
  });

  it.each(["name", "alias", "summary", "icon", "blank"] as const)(
    "opens from the row's %s inside the focusable settings tab",
    async (area) => {
      mount();
      const heading = await screen.findByRole("heading", {
        name: "Fixture group",
      });
      const row = heading.closest("li")!.firstElementChild!
        .firstElementChild as HTMLElement;
      const targets = {
        name: heading,
        alias: within(row).getByText("vary-auto/fixture"),
        summary: within(row).getByText("Fixture provider · fixture-model"),
        icon: within(row).getByRole("img", { name: "Codex" }),
        blank: row,
      };
      fireEvent.click(targets[area]);
      expect(
        screen.getByRole("form", { name: "Fixture group" }),
      ).toBeInTheDocument();
      expect(intentRoutingApi.save).not.toHaveBeenCalled();
    },
  );

  it("does not open a row when its strategy, readiness or delete control is clicked", async () => {
    mount();
    const user = userEvent.setup();
    const strategy = await screen.findByRole("combobox", {
      name: "intentRouting.routingStrategy",
    });
    await user.click(strategy);
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "common.delete" }));
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(screen.getByText("intentRouting.deletePrompt")).toBeInTheDocument();
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

  it("saves edited member weights and free status with the group", async () => {
    mount();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: "common.edit" }),
    );
    await user.click(
      screen.getByRole("combobox", { name: "intentRouting.routingStrategy" }),
    );
    await user.click(
      screen.getByRole("option", { name: "intentRouting.routingWeighted" }),
    );
    fireEvent.change(screen.getByLabelText("intentRouting.memberWeight"), {
      target: { value: "3" },
    });
    await user.click(
      screen.getByRole("combobox", { name: "intentRouting.routingStrategy" }),
    );
    await user.click(
      screen.getByRole("option", { name: "intentRouting.routingFreeFirst" }),
    );
    await user.click(
      screen.getByRole("combobox", { name: "intentRouting.memberFree" }),
    );
    await user.click(
      screen.getByRole("option", { name: "intentRouting.freeYes" }),
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
    await user.click(
      await screen.findByRole("combobox", {
        name: "intentRouting.manualMember",
      }),
    );
    await user.click(screen.getByRole("option", { name: /other-model/ }));
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
    await screen.findByRole("combobox", { name: "intentRouting.manualMember" });
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
      screen.getByRole("combobox", { name: "intentRouting.routingStrategy" }),
    );
    await user.click(
      screen.getByRole("option", { name: "intentRouting.routingManual" }),
    );
    expect(screen.getByLabelText("intentRouting.ruleName")).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "intentRouting.runTrial" }),
    ).toBeDisabled();
    expect(screen.getByLabelText("intentRouting.ruleName")).toHaveValue(
      "Debug",
    );
    expect(screen.getByText("intentRouting.manualPaused")).toBeInTheDocument();
    await user.click(
      screen.getByRole("combobox", { name: "intentRouting.routingStrategy" }),
    );
    await user.click(
      screen.getByRole("option", { name: "intentRouting.routingSmart" }),
    );
    expect(screen.getByLabelText("intentRouting.ruleName")).toBeEnabled();
    expect(screen.getByLabelText("intentRouting.ruleName")).toHaveValue(
      "Debug",
    );
  });

  it("folds unchanged rows but requires saving or cancelling changed drafts", async () => {
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "common.edit" }));
    fireEvent.click(screen.getByRole("button", { name: "Fixture group" }));
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.edit" }));
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Unsaved name" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Fixture group" }));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "intentRouting.saveOrCancel",
    );
    expect(screen.getByLabelText("intentRouting.name")).toHaveValue(
      "Unsaved name",
    );
    expect(intentRoutingApi.save).not.toHaveBeenCalled();
  });

  it("folds the saved group back into its list position with the updated name", async () => {
    const other = { ...group, id: "other", name: "Other group" };
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [group, other],
      autoCreate: false,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.save).mockImplementation(async (value) => {
      const saved = { ...value, revision: value.revision + 1 };
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [saved, other],
        autoCreate: false,
        omittedGroups: 0,
      });
      return saved;
    });
    mount();
    const list = await screen.findByRole("list", {
      name: "intentRouting.title",
    });
    fireEvent.click(
      within(list.children[0] as HTMLElement).getByRole("button", {
        name: "common.edit",
      }),
    );
    expect(
      within(list.children[1] as HTMLElement).getByRole("combobox"),
    ).toBeDisabled();
    expect(
      within(list.children[1] as HTMLElement).getByRole("button", {
        name: "common.delete",
      }),
    ).toBeDisabled();
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "Saved group" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
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

  it("does not show a late plan result in another group's inline editor", async () => {
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
    const list = await screen.findByRole("list", {
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

  it("shows a readable alias and persists the selected scheduling strategy", async () => {
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
    expect(await screen.findByText("auto/fixture-model")).toBeInTheDocument();
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
      screen.getByRole("list", { name: "intentRouting.title" }),
    ).toBeInTheDocument();
  });

  it("refreshes official catalogs before reloading groups", async () => {
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: true,
      omittedGroups: 0,
    });
    vi.mocked(intentRoutingApi.refreshCatalogs).mockResolvedValue([
      { providerId: "provider", status: "cached", modelCount: 3 },
    ]);
    mount();
    fireEvent.click(await screen.findByText("intentRouting.reload"));
    await waitFor(() =>
      expect(intentRoutingApi.refreshCatalogs).toHaveBeenCalledOnce(),
    );
    expect(
      await screen.findByText(/intentRouting.catalogStatus.cached/),
    ).toBeInTheDocument();
  });

  it("keeps persisted discovery enabled when catalog refresh fails", async () => {
    vi.mocked(intentRoutingApi.setAutoCreate).mockImplementation(async () => {
      vi.mocked(intentRoutingApi.list).mockResolvedValue({
        groups: [],
        autoCreate: true,
        omittedGroups: 0,
      });
    });
    vi.mocked(intentRoutingApi.refreshCatalogs).mockRejectedValue(
      "synthetic-private-error",
    );
    mount();
    const toggle = await screen.findByRole("switch", {
      name: "intentRouting.autoCreate",
    });
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(
      screen.queryByText("synthetic-private-error"),
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
    fireEvent.click(await screen.findByText("intentRouting.reload"));
    await waitFor(() =>
      expect(intentRoutingApi.refreshCatalogs).toHaveBeenCalledOnce(),
    );
    panel.unmount();
    mount();
    finish([{ providerId: "provider", status: "updated", modelCount: 1 }]);
    await screen.findByText("intentRouting.reload");
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

  it("loads persisted groups and keeps production activation unavailable", async () => {
    mount();
    expect(await screen.findByText("Fixture group")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Codex" })).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "intentRouting.autoCreate" }),
    ).not.toBeChecked();
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

  it("creates a disabled draft with exact provider and model references", async () => {
    const user = userEvent.setup();
    vi.mocked(intentRoutingApi.list).mockResolvedValue({
      groups: [],
      autoCreate: false,
      omittedGroups: 0,
    });
    mount();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "intentRouting.create" }),
      ).toBeEnabled(),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "intentRouting.create" }),
    );
    fireEvent.change(screen.getByLabelText("intentRouting.name"), {
      target: { value: "New group" },
    });
    const providerSelect = screen.getByRole("combobox", {
      name: "intentRouting.provider",
    });
    expect(providerSelect.tagName).toBe("BUTTON");
    expect(providerSelect).toHaveClass("rounded-full");
    await user.click(providerSelect);
    await user.click(
      await screen.findByRole("option", { name: "Fixture provider" }),
    );
    fireEvent.change(screen.getByLabelText("intentRouting.model"), {
      target: { value: "exact-model" },
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() =>
      expect(intentRoutingApi.save).toHaveBeenCalledWith(
        expect.objectContaining({
          enabled: false,
          revision: 0,
          client: "codex",
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

  it("uses shared selects for every routing field and can clear the classifier", async () => {
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
    for (const name of [
      "provider",
      "defaultTarget",
      "target",
      "classifierProvider",
    ]) {
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

  it("persists automatic discovery and refreshes generated groups", async () => {
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
      screen.getByRole("switch", { name: "intentRouting.autoCreate" }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.setAutoCreate).toHaveBeenCalledWith(true),
    );
    expect(await screen.findByText("shared-model")).toBeInTheDocument();
    expect(screen.getByText("intentRouting.autoGenerated")).toBeInTheDocument();
    expect(screen.getByRole("switch")).toBeChecked();
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
    const providerSelect = screen.getByRole("combobox", {
      name: "intentRouting.provider",
    });
    expect(providerSelect).toHaveTextContent("Official account fixture");
    expect(
      screen.getByText("intentRouting.memberNotReady"),
    ).toBeInTheDocument();
    await user.click(providerSelect);
    await user.click(
      await screen.findByRole("option", { name: "Official account fixture" }),
    );
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

  it("keeps the switch unchanged and offers retry when persistence fails", async () => {
    mount();
    await screen.findByText("Fixture group");
    vi.mocked(intentRoutingApi.setAutoCreate).mockRejectedValue(
      "private failure",
    );
    fireEvent.click(
      screen.getByRole("switch", { name: "intentRouting.autoCreate" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "intentRouting.failed",
    );
    expect(screen.getByRole("switch")).not.toBeChecked();
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
