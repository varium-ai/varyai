import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { RoutingUpstreamsPanel } from "@/components/providers/RoutingUpstreamsPanel";
import {
  routingUpstreamsApi,
  type RoutingUpstream,
} from "@/lib/api/routingUpstreams";

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(),
    save: vi.fn(),
    editor: vi.fn(),
    remove: vi.fn(),
  },
}));

const storageKey = "vary-provider-order";
const fixtures: RoutingUpstream[] = [
  "alpha",
  "beta",
  "gamma",
  "off-one",
  "off-two",
].map((id) => ({
  schemaVersion: 1,
  id,
  name: id,
  revision: 1,
  hasApiKey: true,
  enabled: !id.startsWith("off-"),
  source: {
    kind: "api",
    baseUrl: "https://example.test/v1",
    apiFormat: "openai_chat",
    models: id === "beta" ? [] : ["shared-fixture"],
    catalog: [],
    mode: "vendor",
    groupsOnly: false,
  },
}));
function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return {
    client,
    ...render(
      <QueryClientProvider client={client}>
        <RoutingUpstreamsPanel />
      </QueryClientProvider>,
    ),
  };
}
const order = () =>
  screen
    .getAllByRole("article")
    .map((row) => row.getAttribute("data-provider-id"));
const handle = (id: string) =>
  within(screen.getByRole("article", { name: id })).getByRole("button", {
    name: "modelProviders.sorting.handle",
  });
function drag(id: string, from: number, to: number) {
  fireEvent.pointerDown(handle(id), { button: 0, clientX: 12, clientY: from });
  fireEvent.pointerMove(document, { clientX: 12, clientY: from + 10 });
  fireEvent.pointerMove(document, { clientX: 12, clientY: to });
}
beforeEach(() => {
  vi.clearAllMocks();
  localStorage.removeItem(storageKey);
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue(fixtures);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      const row = this.closest(".pv-row") ?? this;
      const rows = Array.from(
        row.parentElement?.querySelectorAll(".pv-row") ?? [],
      );
      const top = Math.max(0, rows.indexOf(row)) * 72;
      return {
        x: 0,
        y: top,
        left: 0,
        top,
        right: 800,
        bottom: top + 72,
        width: 800,
        height: 72,
        toJSON: () => ({}),
      };
    },
  );
  vi.stubGlobal(
    "PointerEvent",
    class extends MouseEvent {
      pointerId = 1;
      isPrimary = true;
      pointerType = "mouse";
    },
  );
});
afterEach(async () => {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 60));
  });
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  localStorage.removeItem(storageKey);
});

it("reorders providers from their handles, persists on remount, and never changes configuration", async () => {
  const { unmount } = mount();
  await screen.findByRole("article", { name: "beta" });
  drag("beta", 108, 36);
  fireEvent.pointerUp(document, { clientX: 12, clientY: 36 });
  await waitFor(() => expect(order()).toEqual(["beta", "alpha", "gamma"]));
  expect(JSON.parse(localStorage.getItem(storageKey)!)).toEqual([
    "beta",
    "alpha",
    "gamma",
    "off-one",
    "off-two",
  ]);
  expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  unmount();
  mount();
  await screen.findByRole("article", { name: "beta" });
  expect(order()).toEqual(["beta", "alpha", "gamma"]);
});

it("only sorts from the handle and cancels without opening the editor or saving", async () => {
  mount();
  const row = await screen.findByRole("article", { name: "beta" });
  fireEvent.pointerDown(within(row).getByText("beta"), {
    button: 0,
    clientX: 120,
    clientY: 108,
  });
  fireEvent.pointerMove(document, { clientX: 120, clientY: 36 });
  fireEvent.pointerUp(document);
  await userEvent.click(handle("beta"));
  expect(order()).toEqual(["alpha", "beta", "gamma"]);
  drag("beta", 108, 36);
  await act(async () => fireEvent.pointerCancel(document));
  expect(order()).toEqual(["alpha", "beta", "gamma"]);
  expect(localStorage.getItem(storageKey)).toBeNull();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

it("supports keyboard sorting and preserves focus", async () => {
  mount();
  await screen.findByRole("article", { name: "beta" });
  handle("beta").focus();
  await userEvent.keyboard("[Space][ArrowUp][Space]");
  await waitFor(() => expect(order()).toEqual(["beta", "alpha", "gamma"]));
  expect(handle("beta")).toHaveFocus();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

it("preserves providers hidden by search when reordering visible results", async () => {
  mount();
  await screen.findByRole("article", { name: "gamma" });
  fireEvent.change(screen.getByRole("searchbox"), {
    target: { value: "shared-fixture" },
  });
  drag("gamma", 108, 36);
  fireEvent.pointerUp(document, { clientX: 12, clientY: 36 });
  await waitFor(() => expect(order().slice(0, 2)).toEqual(["gamma", "alpha"]));
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "" } });
  expect(order()).toEqual(["gamma", "beta", "alpha"]);
  expect(JSON.parse(localStorage.getItem(storageKey)!)).toContain("off-one");
});

it("sorts switched-off providers independently and retains that order after enabling", async () => {
  const { client } = mount();
  await screen.findByRole("article", { name: "alpha" });
  fireEvent.click(
    screen.getByRole("button", { name: "modelProviders.turnedOff" }),
  );
  handle("off-two").focus();
  await userEvent.keyboard("[Space][ArrowUp][Space]");
  await waitFor(() =>
    expect(order()).toEqual(["alpha", "beta", "gamma", "off-two", "off-one"]),
  );
  act(() =>
    client.setQueryData(
      ["routing-upstreams"],
      fixtures.map((row) => ({ ...row, enabled: true })),
    ),
  );
  expect(order()).toEqual(["alpha", "beta", "gamma", "off-two", "off-one"]);
  expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
});

it("validates saved IDs and appends new providers without losing the existing order", async () => {
  localStorage.setItem(
    storageKey,
    JSON.stringify(["gamma", "deleted", "gamma", 3, "alpha"]),
  );
  const { client } = mount();
  await screen.findByRole("article", { name: "alpha" });
  expect(order()).toEqual(["gamma", "alpha", "beta"]);
  act(() =>
    client.setQueryData(
      ["routing-upstreams"],
      [...fixtures, { ...fixtures[0], id: "new", name: "new" }],
    ),
  );
  await waitFor(() =>
    expect(order()).toEqual(["gamma", "alpha", "beta", "new"]),
  );
});

it("keeps sorting usable in this session when storage cannot be written", async () => {
  mount();
  await screen.findByRole("article", { name: "beta" });
  vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("Storage unavailable");
  });
  drag("beta", 108, 36);
  fireEvent.pointerUp(document, { clientX: 12, clientY: 36 });
  await waitFor(() => expect(order()).toEqual(["beta", "alpha", "gamma"]));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
