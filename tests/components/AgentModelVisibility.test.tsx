import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { AgentModelVisibility } from "@/components/agents/AgentModelVisibility";
import {
  agentConnectionsApi,
  type AgentModelVisibility as Visibility,
} from "@/lib/api/agentConnections";

vi.mock("@/lib/api/agentConnections", () => ({
  agentConnectionsApi: {
    modelVisibility: vi.fn(),
    setModelVisibility: vi.fn(),
  },
}));

const models = [
  {
    id: "route",
    name: "Smart · routing group",
    group: "routing",
    contextWindow: 1_000_000,
  },
  {
    id: "a",
    name: "Alpha · Vendor one",
    group: "Vendor one",
    contextWindow: 200_000,
    images: true,
    reasoningLevels: ["low", "high"],
  },
  {
    id: "b",
    name: "Beta · Vendor one",
    group: "Vendor one",
    contextWindow: null,
  },
  {
    id: "c",
    name: "Gamma · Vendor two",
    group: "Vendor two",
    contextWindow: null,
  },
];
const busy = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(agentConnectionsApi.modelVisibility).mockResolvedValue({
    only: false,
    ids: [],
  });
  vi.mocked(agentConnectionsApi.setModelVisibility).mockResolvedValue();
});

async function open() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <AgentModelVisibility
        app="claude-desktop"
        name="Desktop"
        connected
        models={models}
        disabled={false}
        onBusyChange={busy}
      />
      <button>Outside</button>
    </QueryClientProvider>,
  );
  const trigger = screen.getByRole("button", {
    name: "agentConnections.modelList Desktop",
  });
  await waitFor(() => expect(trigger).toBeEnabled());
  fireEvent.click(trigger);
  return { trigger, client, dialog: await screen.findByRole("dialog") };
}

it("uses the reference implementation's compact immediate-save model list with inline provider suffixes and without form actions", async () => {
  const { dialog, trigger } = await open();
  expect(dialog).toHaveClass("mp-model-picker-popover");
  expect(dialog.querySelector(".mp-shared-picker .mp-am-body")).not.toBeNull();
  expect(
    within(dialog).queryByRole("button", { name: "common.save" }),
  ).not.toBeInTheDocument();
  expect(
    within(dialog).queryByRole("button", { name: "common.cancel" }),
  ).not.toBeInTheDocument();
  const alpha = within(dialog).getByRole("menuitemcheckbox", {
    name: "Alpha · Vendor one",
  });
  expect(alpha).toHaveAttribute("aria-checked", "true");
  expect(
    within(dialog).getByRole("button", {
      name: "agentConnections.modelViewShown",
    }),
  ).toBeInTheDocument();
  expect(
    within(dialog).getByRole("button", {
      name: "agentConnections.modelViewOrder",
    }),
  ).toBeInTheDocument();
  fireEvent.click(alpha);
  await waitFor(() =>
    expect(agentConnectionsApi.setModelVisibility).toHaveBeenCalledWith(
      "claude-desktop",
      { only: false, ids: ["a"] },
    ),
  );
  expect(alpha).toHaveAttribute("aria-checked", "false");
  expect(dialog).toBeInTheDocument();
  expect(trigger).toHaveTextContent("agentConnections.shownModels");
  fireEvent.change(within(dialog).getByRole("searchbox"), {
    target: { value: "Alpha" },
  });
  fireEvent.keyDown(within(dialog).getByRole("searchbox"), { key: "Escape" });
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
});

it("uses empty maker slots and context text rather than repeating provider initials or capability badges", async () => {
  const { dialog } = await open();
  const alpha = within(dialog).getByRole("menuitemcheckbox", {
    name: "Alpha · Vendor one",
  });
  expect(alpha.querySelector(".mp-am-logo")).toBeEmptyDOMElement();
  expect(alpha).toHaveTextContent("200K");
  expect(alpha.querySelector(".mp-model-badge")).toBeNull();
  expect(within(alpha).queryByRole("img")).not.toBeInTheDocument();
});

it("shows routing, relay and subscription groups from the shared catalog on the provider rail", async () => {
  const catalog = [
    ...Array.from({ length: 8 }, (_, index) => ({
      id: `route/${index}`,
      name: `Shared ${index} · routing group`,
      group: "routing",
      contextWindow: 500_000,
    })),
    ...Array.from({ length: 10 }, (_, index) => ({
      id: `relay/${index}`,
      name: `Relay ${index} · Relay`,
      group: "Relay",
      contextWindow: null,
    })),
    ...Array.from({ length: 7 }, (_, index) => ({
      id: `vendor/${index}`,
      name: `Vendor ${index} · Vendor`,
      group: "Vendor",
      contextWindow: null,
    })),
    ...Array.from({ length: 8 }, (_, index) => ({
      id: `subscription/${index}`,
      name: `Shared ${index} · Subscription`,
      group: "Subscription",
      contextWindow: 500_000,
    })),
  ];
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AgentModelVisibility
        app="claude-desktop"
        name="Desktop"
        connected
        models={catalog}
        disabled={false}
        onBusyChange={busy}
      />
    </QueryClientProvider>,
  );
  const trigger = screen.getByRole("button", {
    name: "agentConnections.modelList Desktop",
  });
  await waitFor(() => expect(trigger).toBeEnabled());
  fireEvent.click(trigger);
  const dialog = await screen.findByRole("dialog");
  expect(dialog.querySelector(".mp-shared-picker")).toHaveClass("railed");
  const rail = within(dialog).getByRole("navigation");
  for (const [name, count] of [
    ["agentConnections.routingGroups", 8],
    ["Relay", 10],
    ["Vendor", 7],
    ["Subscription", 8],
  ] as const) {
    const button = within(rail).getByRole("button", { name });
    expect(button).toHaveTextContent(`${count}/${count}`);
    fireEvent.click(button);
    expect(within(dialog).getAllByRole("menuitemcheckbox")).toHaveLength(count);
  }
  expect(
    within(dialog).getByRole("menuitemcheckbox", {
      name: "Shared 0 · Subscription",
    }),
  ).toHaveTextContent("500K");
});

it("searches all providers even after choosing one on the rail and retains just-hidden rows in Shown", async () => {
  const { dialog } = await open();
  fireEvent.click(
    within(within(dialog).getByRole("navigation")).getByRole("button", {
      name: "Vendor one",
    }),
  );
  expect(
    within(dialog).queryByRole("menuitemcheckbox", {
      name: "Gamma · Vendor two",
    }),
  ).not.toBeInTheDocument();
  fireEvent.change(within(dialog).getByRole("searchbox"), {
    target: { value: "Gamma" },
  });
  expect(
    within(dialog).getByRole("menuitemcheckbox", {
      name: "Gamma · Vendor two",
    }),
  ).toBeInTheDocument();
  fireEvent.change(within(dialog).getByRole("searchbox"), {
    target: { value: "" },
  });
  fireEvent.click(
    within(dialog).getByRole("button", {
      name: "agentConnections.modelViewShown",
    }),
  );
  fireEvent.click(
    within(dialog).getByRole("menuitemcheckbox", {
      name: "Alpha · Vendor one",
    }),
  );
  expect(
    within(dialog).getByRole("menuitemcheckbox", {
      name: "Alpha · Vendor one",
    }),
  ).toHaveAttribute("aria-checked", "false");
  fireEvent.click(
    within(dialog).getByRole("button", {
      name: "agentConnections.modelViewAll",
    }),
  );
  fireEvent.click(
    within(dialog).getByRole("button", {
      name: "agentConnections.modelViewShown",
    }),
  );
  expect(
    within(dialog).queryByRole("menuitemcheckbox", {
      name: "Alpha · Vendor one",
    }),
  ).not.toBeInTheDocument();
});

it("requires a second click for a global bulk action and saves only-picked without changing the subset", async () => {
  const { dialog } = await open();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "agentConnections.hideAll" }),
  );
  expect(agentConnectionsApi.setModelVisibility).not.toHaveBeenCalled();
  fireEvent.click(
    within(dialog).getByRole("button", {
      name: "agentConnections.confirmHideAll",
    }),
  );
  await waitFor(() =>
    expect(agentConnectionsApi.setModelVisibility).toHaveBeenLastCalledWith(
      "claude-desktop",
      { only: false, ids: ["route", "a", "b", "c"] },
    ),
  );
  fireEvent.click(
    within(dialog).getByRole("switch", { name: "agentConnections.onlyPicked" }),
  );
  await waitFor(() =>
    expect(agentConnectionsApi.setModelVisibility).toHaveBeenLastCalledWith(
      "claude-desktop",
      { only: true, ids: [] },
    ),
  );
});

it("serializes rapid choices and restores the latest saved state on failure without leaking errors", async () => {
  let finish!: () => void;
  vi.mocked(agentConnectionsApi.setModelVisibility)
    .mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    )
    .mockRejectedValueOnce("synthetic-private-error");
  const { dialog, client } = await open();
  fireEvent.click(
    within(dialog).getByRole("menuitemcheckbox", {
      name: "Alpha · Vendor one",
    }),
  );
  fireEvent.click(
    within(dialog).getByRole("menuitemcheckbox", { name: "Beta · Vendor one" }),
  );
  await waitFor(() =>
    expect(agentConnectionsApi.setModelVisibility).toHaveBeenCalledTimes(1),
  );
  await act(async () => finish());
  await waitFor(() =>
    expect(agentConnectionsApi.setModelVisibility).toHaveBeenCalledTimes(2),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "agentConnections.failed",
  );
  expect(
    within(dialog).getByRole("menuitemcheckbox", {
      name: "Alpha · Vendor one",
    }),
  ).toHaveAttribute("aria-checked", "false");
  expect(
    within(dialog).getByRole("menuitemcheckbox", { name: "Beta · Vendor one" }),
  ).toHaveAttribute("aria-checked", "true");
  expect(
    client.getQueryData<Visibility>([
      "agent-model-visibility",
      "claude-desktop",
    ]),
  ).toEqual({ only: false, ids: ["a"] });
  expect(dialog).not.toHaveTextContent("synthetic-private-error");
});

it("saves keyboard model ordering and restores the default order", async () => {
  const { dialog } = await open();
  fireEvent.click(
    within(dialog).getByRole("button", {
      name: "agentConnections.modelViewOrder",
    }),
  );
  const alpha = within(dialog).getByRole("listitem", {
    name: "Alpha · Vendor one",
  });
  fireEvent.keyDown(alpha, { altKey: true, key: "ArrowUp" });
  await waitFor(() =>
    expect(agentConnectionsApi.setModelVisibility).toHaveBeenLastCalledWith(
      "claude-desktop",
      { only: false, ids: [], order: ["a", "route", "b", "c"] },
    ),
  );
  expect(within(dialog).getAllByRole("listitem")[0]).toHaveAccessibleName(
    "Alpha · Vendor one",
  );
  fireEvent.click(
    within(dialog).getByRole("button", {
      name: "agentConnections.defaultModelOrder",
    }),
  );
  await waitFor(() =>
    expect(agentConnectionsApi.setModelVisibility).toHaveBeenLastCalledWith(
      "claude-desktop",
      { only: false, ids: [] },
    ),
  );
});

it("supports keyboard model choices and closes on outside scrolling without discarding queued saves", async () => {
  const { dialog, trigger } = await open();
  fireEvent.keyDown(within(dialog).getByRole("searchbox"), {
    key: "ArrowDown",
  });
  expect(
    within(dialog).getByRole("menuitemcheckbox", {
      name: "Smart · routing group",
    }),
  ).toHaveFocus();
  fireEvent.scroll(
    within(dialog).getByRole("menuitemcheckbox", {
      name: "Smart · routing group",
    }),
  );
  expect(dialog).toBeInTheDocument();
  fireEvent.click(
    within(dialog).getByRole("menuitemcheckbox", {
      name: "Alpha · Vendor one",
    }),
  );
  fireEvent.scroll(document);
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  await waitFor(() =>
    expect(agentConnectionsApi.setModelVisibility).toHaveBeenCalledWith(
      "claude-desktop",
      { only: false, ids: ["a"] },
    ),
  );
  fireEvent.click(trigger);
  const reopened = await screen.findByRole("dialog");
  expect(
    within(reopened).getByRole("menuitemcheckbox", {
      name: "Alpha · Vendor one",
    }),
  ).toHaveAttribute("aria-checked", "false");
});
