import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import {
  finishDialogAnimation,
  mockDialogAnimationStyles,
} from "../utils/dialogAnimation";
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
  expect(
    screen.queryByLabelText("modelProviders.baseUrl"),
  ).not.toBeInTheDocument();
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
  expect(
    screen.queryByRole("button", { name: "routingUpstreams.import" }),
  ).not.toBeInTheDocument();
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
    screen.getByRole("heading", { level: 1, name: "providerFlow.sheet" }),
  ).toBeVisible();
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
    screen.getByRole("heading", { level: 1, name: "providerFlow.sheet" }),
  ).toBeVisible();
  const back = screen.getByRole("button", { name: "common.back" });
  expect(back.closest("header")).toContainElement(
    screen.getByRole("heading", { level: 1, name: "providerFlow.sheet" }),
  );
  expect(
    screen.getByRole("button", { name: "routingUpstreams.import" }),
  ).toBeVisible();
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
  fireEvent.click(within(editor).getByRole("button", { name: "common.add" }));
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
it("imports from the add page and restores focus to its import card", async () => {
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
  mount();
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  const importButton = screen.getByRole("button", {
    name: "routingUpstreams.import",
  });
  await waitFor(() => expect(importButton).toBeEnabled());
  importButton.focus();
  fireEvent.click(importButton);
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
    screen.getByRole("region", { name: "providerFlow.sheet" }),
  ).toBeVisible();
  expect(importButton).toHaveFocus();
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
it("keeps preset endpoints read-only and saves the default transport with the entered key", async () => {
  mount(true);
  fireEvent.click(screen.getByRole("button", { name: "DeepSeek" }));
  fireEvent.change(screen.getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-key" },
  });
  expect(
    screen.queryByRole("radio", { name: "Anthropic" }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByLabelText("modelProviders.baseUrl"),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "common.add" }));
  await waitFor(() =>
    expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
      expect.objectContaining({
        source: expect.objectContaining({
          apiFormat: "openai_chat",
          baseUrl: "https://api.deepseek.com/v1",
          apiKey: "synthetic-key",
        }),
      }),
    ),
  );
});

it("pairs a vendor's global and China sites and preserves the unsaved key while changing region", async () => {
  mount();
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.vendor" }));
  fireEvent.click(screen.getByRole("button", { name: /^Kimi$/ }));
  fireEvent.change(screen.getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-key" },
  });
  expect(
    screen.queryByLabelText("modelProviders.baseUrl"),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("radio", { name: "providerFlow.china" }));
  expect(
    screen.queryByLabelText("modelProviders.baseUrl"),
  ).not.toBeInTheDocument();
  expect(screen.getByLabelText("modelProviders.apiKey")).toHaveValue(
    "synthetic-key",
  );
  expect(
    screen.getByRole("dialog", { name: "Kimi (China)" }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "common.add" }));
  await waitFor(() =>
    expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
      expect.objectContaining({
        region: "china",
        source: expect.objectContaining({
          baseUrl: "https://api.moonshot.cn/v1",
          apiKey: "synthetic-key",
        }),
      }),
    ),
  );
});

it("uses Z.ai's Coding Plan endpoints without exposing the separate endpoint editor", async () => {
  mount(true);
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.vendor" }));
  fireEvent.click(screen.getByRole("button", { name: "Z.ai" }));
  fireEvent.change(screen.getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-key" },
  });
  fireEvent.click(screen.getByRole("radio", { name: "Coding Plan" }));
  expect(
    screen.queryByLabelText("modelProviders.baseUrl"),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("radio", { name: "Responses" }),
  ).not.toBeInTheDocument();
  expect(screen.getByLabelText("modelProviders.apiKey")).toHaveValue(
    "synthetic-key",
  );
  fireEvent.click(screen.getByRole("button", { name: "common.add" }));
  await waitFor(() =>
    expect(routingUpstreamsApi.save).toHaveBeenCalledWith(
      expect.objectContaining({
        region: "coding",
        source: expect.objectContaining({
          apiFormat: "openai_chat",
          baseUrl: "https://api.z.ai/api/coding/paas/v4",
          apiKey: "synthetic-key",
        }),
      }),
    ),
  );
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

it("offers managed subscription accounts separately from API vendors and searches their aliases", async () => {
  mount(true);
  for (const name of [
    "Claude",
    "ChatGPT / Codex",
    "Gemini CLI",
    "Antigravity",
  ]) {
    expect(screen.getByRole("button", { name })).toBeVisible();
  }
  const search = screen.getByRole("searchbox", {
    name: "providerFlow.findVendor",
  });
  fireEvent.change(search, { target: { value: "supergrok" } });
  expect(screen.getByRole("button", { name: "Grok" })).toBeVisible();
  expect(screen.queryByText("providerFlow.noVendor")).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "GitHub Copilot" }),
  ).not.toBeInTheDocument();
});

it("offers four setup methods and excludes the API relay group", () => {
  mount(true);
  expect(
    screen.getByRole("heading", { level: 1, name: "providerFlow.sheet" }),
  ).toBeVisible();
  for (const name of [
    "providerParity.subscription",
    "providerFlow.vendor",
    "routingUpstreams.import",
    "providerFlow.custom",
  ]) {
    expect(screen.getByRole("button", { name })).toBeEnabled();
  }
  expect(
    screen.getByRole("heading", { name: "providerFlow.commonProviders" }),
  ).toBeVisible();
  expect(screen.queryByText("providerFlow.relay")).not.toBeInTheDocument();
  for (const name of ["OpenRouter", "SiliconFlow", "ModelScope"]) {
    expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
  }
});

it("cancels shortcut sign-in before importing", async () => {
  vi.mocked(providerImportApi.preview).mockResolvedValue([]);
  mount(true);
  fireEvent.click(screen.getByRole("button", { name: "Claude" }));
  expect(
    screen.getByRole("region", { name: "subscriptionLogin.title" }),
  ).toBeVisible();
  const trigger = screen.getByRole("button", {
    name: "routingUpstreams.import",
  });
  trigger.focus();
  fireEvent.click(trigger);
  expect(
    screen.queryByRole("region", { name: "subscriptionLogin.title" }),
  ).not.toBeInTheDocument();
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(
    within(dialog).getByRole("button", { name: "common.cancel" }),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(trigger).toHaveFocus();
  expect(
    screen.getByRole("region", { name: "providerFlow.sheet" }),
  ).toBeVisible();
});

it.each(["providerParity.subscription", "providerFlow.vendor"])(
  "opens %s in a modal and restores the page and trigger focus on close",
  async (name) => {
    mount(true);
    const page = screen.getByRole("region", { name: "providerFlow.sheet" });
    const search = screen.getByRole("searchbox", {
      name: "providerFlow.findVendor",
    });
    fireEvent.change(search, { target: { value: "gemini" } });
    const trigger = screen.getByRole("button", { name });
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name });
    expect(within(dialog).getByRole("searchbox")).toHaveFocus();
    expect(page.querySelectorAll(".pv-add-method")).toHaveLength(4);
    expect(page.querySelector(".pv-common-providers")).toBeInTheDocument();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "common.close" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name })).not.toBeInTheDocument(),
    );
    expect(search).toHaveValue("gemini");
    expect(trigger).toHaveFocus();
  },
);

it("keeps four shortcuts per row and browses the full vendor directory in a dialog", async () => {
  mount(true);
  const page = screen.getByRole("region", { name: "providerFlow.sheet" });
  for (const name of ["providerParity.subscription", "providerFlow.vendor"]) {
    const row = screen
      .getByRole("heading", { name })
      .closest(".pv-vendors") as HTMLElement;
    expect(within(row).getAllByRole("button")).toHaveLength(4);
  }
  expect(
    screen.queryByRole("button", { name: "Kimi" }),
  ).not.toBeInTheDocument();
  const trigger = screen.getByRole("button", { name: "providerFlow.vendor" });
  fireEvent.click(trigger);
  const dialog = screen.getByRole("dialog", { name: "providerFlow.vendor" });
  expect(within(dialog).getByRole("button", { name: "Kimi" })).toBeVisible();
  expect(page.querySelectorAll(".pv-tile")).toHaveLength(8);
  const search = within(dialog).getByRole("searchbox");
  fireEvent.change(search, { target: { value: "moonshot.cn" } });
  expect(within(dialog).getByRole("button", { name: "Kimi" })).toBeVisible();
  expect(
    within(dialog).queryByRole("button", { name: "DeepSeek" }),
  ).not.toBeInTheDocument();
  fireEvent.keyDown(search, { key: "Escape" });
  expect(search).toHaveValue("");
  expect(dialog).toBeVisible();
  fireEvent.keyDown(search, { key: "Escape" });
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(
    screen.queryByRole("button", { name: "Kimi" }),
  ).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

it("keeps all subscription providers available inside the subscription dialog", async () => {
  mount(true);
  const trigger = screen.getByRole("button", {
    name: "providerParity.subscription",
  });
  fireEvent.click(trigger);
  const dialog = screen.getByRole("dialog", {
    name: "providerParity.subscription",
  });
  for (const name of ["GitHub Copilot", "ChatGPT API", "Grok"]) {
    expect(within(dialog).getByRole("button", { name })).toBeVisible();
  }
  expect(
    within(dialog).queryByRole("button", { name: "DeepSeek" }),
  ).not.toBeInTheDocument();
  fireEvent.keyDown(within(dialog).getByRole("searchbox"), { key: "Escape" });
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(screen.getByRole("button", { name: "DeepSeek" })).toBeVisible();
  expect(trigger).toHaveFocus();
});

it("keeps focus in the API editor after the picker exit animation finishes", async () => {
  const style = mockDialogAnimationStyles();
  try {
    mount(true);
    fireEvent.click(
      screen.getByRole("button", { name: "providerFlow.vendor" }),
    );
    const picker = screen.getByRole("dialog", { name: "providerFlow.vendor" });
    fireEvent.click(within(picker).getByRole("button", { name: "Kimi" }));
    const editor = screen.getByRole("dialog", { name: "Kimi" });
    const key = within(editor).getByLabelText("modelProviders.apiKey");
    expect(key).toHaveFocus();
    finishDialogAnimation(picker);
    await waitFor(() => expect(picker).not.toBeInTheDocument());
    expect(key).toHaveFocus();
  } finally {
    style.mockRestore();
    vi.unstubAllGlobals();
  }
});
