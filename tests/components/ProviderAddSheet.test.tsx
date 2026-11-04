import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { RoutingUpstreamsPanel } from "@/components/providers/RoutingUpstreamsPanel";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import { providerImportApi } from "@/lib/api/providerImport";

vi.mock("@/lib/api/providerImport", () => ({
  providerImportApi: { preview: vi.fn(), import: vi.fn() },
}));

vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(),
    sources: vi.fn(),
    import: vi.fn(),
    save: vi.fn(),
    remove: vi.fn(),
  },
}));
beforeEach(() => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([]);
});
function mount(startAdding = false) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <RoutingUpstreamsPanel startAdding={startAdding} />
    </QueryClientProvider>,
  );
}
it("opens the provider selection page before opening a provider editor", async () => {
  mount();
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  fireEvent.change(
    screen.getByRole("searchbox", { name: "providerFlow.findVendor" }),
    { target: { value: "deepseek" } },
  );
  fireEvent.click(screen.getByRole("button", { name: "DeepSeek" }));
  expect(
    await screen.findByRole("region", { name: "DeepSeek" }),
  ).toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "DeepSeek" })).toBeInTheDocument();
  expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
    "https://api.deepseek.com/v1",
  );
  expect(
    screen.queryByLabelText("modelProviders.name"),
  ).not.toBeInTheDocument();
  expect(screen.getByLabelText("modelProviders.apiKey")).toHaveFocus();
});
it("moves Add provider to the page header and returns to the filtered list with focus restored", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    {
      schemaVersion: 1,
      id: "fixture",
      name: "Synthetic provider",
      revision: 1,
      hasApiKey: true,
      source: {
        kind: "api",
        baseUrl: "https://provider.example/v1",
        apiFormat: "openai_chat",
        models: ["fixture-model"],
        catalog: [],
        mode: "vendor",
        groupsOnly: false,
      },
    },
  ]);
  mount();
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  expect(add.closest("header")).toContainElement(
    screen.getByRole("heading", { level: 1, name: "modelProviders.title" }),
  );
  fireEvent.change(
    screen.getByRole("searchbox", { name: "modelProviders.find" }),
    {
      target: { value: "Synthetic" },
    },
  );
  fireEvent.click(add);
  expect(
    screen.queryByRole("heading", { name: "modelProviders.title" }),
  ).not.toBeInTheDocument();
  expect(screen.queryByRole("article")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("searchbox", { name: "modelProviders.find" }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole("heading", { level: 1, name: "modelProviders.add" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("searchbox", { name: "providerFlow.findVendor" }),
  ).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "common.back" }));
  expect(
    screen.queryByRole("region", { name: "providerFlow.sheet" }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole("article", { name: "Synthetic provider" }),
  ).toBeVisible();
  expect(
    screen.getByRole("searchbox", { name: "modelProviders.find" }),
  ).toHaveValue("Synthetic");
  expect(
    screen.getByRole("button", { name: "modelProviders.add" }),
  ).toHaveFocus();
  expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
});
it("opens directly on the add page when requested by another workspace page", async () => {
  mount(true);
  expect(
    screen.getByRole("searchbox", { name: "providerFlow.findVendor" }),
  ).toBeVisible();
  expect(
    screen.getByRole("heading", { level: 1, name: "providerFlow.first" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("heading", { name: "modelProviders.title" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "common.back" }));
  expect(
    screen.getByRole("heading", { name: "modelProviders.title" }),
  ).toBeInTheDocument();
});
it("returns to the list after saving a new provider", async () => {
  vi.mocked(routingUpstreamsApi.save).mockResolvedValue({
    id: "created",
    revision: 1,
  } as never);
  mount(true);
  fireEvent.click(screen.getByRole("button", { name: "DeepSeek" }));
  const editor = screen.getByRole("dialog", { name: "DeepSeek" });
  fireEvent.change(within(editor).getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-key" },
  });
  fireEvent.click(within(editor).getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(
    screen.getByRole("heading", { name: "modelProviders.title" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("region", { name: "providerFlow.sheet" }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "modelProviders.add" }),
  ).toHaveFocus();
});
it("returns to the list after importing and restores the header action's focus", async () => {
  vi.mocked(providerImportApi.preview).mockResolvedValue([
    {
      id: "magpie",
      name: "Magpie",
      found: true,
      items: [
        {
          reference: "fixture",
          fingerprint: "synthetic-fingerprint",
          name: "Synthetic imported provider",
          host: "provider.example",
          modelCount: 1,
          status: "new",
          enabled: true,
        },
      ],
    },
  ] as never);
  vi.mocked(providerImportApi.import).mockResolvedValue({
    added: 1,
    skipped: 0,
  });
  mount(true);
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.import" }));
  const dialog = await screen.findByRole("dialog");
  expect(
    await within(dialog).findByText("Synthetic imported provider"),
  ).toBeVisible();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "providerFlow.import" }),
  );
  await waitFor(() => expect(providerImportApi.import).toHaveBeenCalledOnce());
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(
    screen.queryByRole("region", { name: "providerFlow.sheet" }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "modelProviders.add" }),
  ).toHaveFocus();
});
it("requires confirmation before discarding edits and returns to the add page", async () => {
  mount();
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.custom" }));
  expect(screen.getByLabelText("modelProviders.name")).toHaveFocus();
  fireEvent.change(screen.getByLabelText("modelProviders.name"), {
    target: { value: "Unsaved" },
  });
  fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
  expect(
    await screen.findByText("providerFlow.discardTitle"),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.discard" }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(
    screen.getByRole("searchbox", { name: "providerFlow.findVendor" }),
  ).toBeInTheDocument();
});
it("switches a vendor's protocol to that protocol's own endpoint and keeps the entered key", async () => {
  mount();
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  fireEvent.click(screen.getByRole("button", { name: "DeepSeek" }));
  fireEvent.change(screen.getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-key" },
  });
  fireEvent.click(screen.getByRole("radio", { name: "Anthropic" }));
  expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
    "https://api.deepseek.com/anthropic",
  );
  expect(screen.getByLabelText("modelProviders.apiKey")).toHaveValue(
    "synthetic-key",
  );
  fireEvent.click(screen.getByRole("radio", { name: "Chat Completions" }));
  expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
    "https://api.deepseek.com/v1",
  );
});

it("pairs a vendor's global and China sites and preserves the unsaved key while changing region", async () => {
  mount();
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  fireEvent.click(screen.getByRole("button", { name: /^Kimi$/ }));
  fireEvent.change(screen.getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-key" },
  });
  expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
    "https://api.moonshot.ai/v1",
  );
  fireEvent.click(screen.getByRole("radio", { name: "providerFlow.china" }));
  expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
    "https://api.moonshot.cn/v1",
  );
  expect(screen.getByLabelText("modelProviders.apiKey")).toHaveValue(
    "synthetic-key",
  );
  expect(
    screen.getByRole("dialog", { name: "Kimi (China)" }),
  ).toBeInTheDocument();
});

it("uses Z.ai's separate Coding Plan endpoints and retains its key when changing protocols", async () => {
  mount();
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  fireEvent.click(screen.getByRole("button", { name: "Z.ai" }));
  fireEvent.change(screen.getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-key" },
  });
  fireEvent.click(screen.getByRole("radio", { name: "Coding Plan" }));
  expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
    "https://api.z.ai/api/coding/paas/v4",
  );
  fireEvent.click(screen.getByRole("radio", { name: "Responses" }));
  expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
    "https://api.z.ai/api/v1",
  );
  expect(screen.getByLabelText("modelProviders.apiKey")).toHaveValue(
    "synthetic-key",
  );
  fireEvent.click(screen.getByRole("radio", { name: "Anthropic" }));
  expect(screen.getByLabelText("modelProviders.baseUrl")).toHaveValue(
    "https://api.z.ai/api/anthropic",
  );
  fireEvent.click(screen.getByRole("radio", { name: "Pay as you go" }));
  expect(
    screen.queryByRole("radio", { name: "Responses" }),
  ).not.toBeInTheDocument();
});
it("clears the vendor search with Escape before returning to the list", async () => {
  mount();
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  const search = screen.getByRole("searchbox", {
    name: "providerFlow.findVendor",
  });
  fireEvent.change(search, { target: { value: "moonshot" } });
  expect(
    screen.queryByRole("button", { name: "DeepSeek" }),
  ).not.toBeInTheDocument();
  fireEvent.keyDown(search, { key: "Escape" });
  expect(search).toHaveValue("");
  expect(screen.getByRole("button", { name: "DeepSeek" })).toBeInTheDocument();
  fireEvent.keyDown(search, { key: "Escape" });
  expect(
    screen.queryByRole("region", { name: "providerFlow.sheet" }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "modelProviders.add" }),
  ).toHaveFocus();
});
