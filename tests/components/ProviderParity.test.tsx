import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { RoutingUpstreamsPanel } from "@/components/providers/RoutingUpstreamsPanel";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
vi.mock("@/lib/api/model-fetch", () => ({ fetchModelsForConfig: vi.fn() }));
vi.mock("@/lib/api/routingUpstreams", () => ({
  routingUpstreamsApi: {
    list: vi.fn(),
    save: vi.fn(),
    discover: vi.fn(),
    remove: vi.fn(),
    editor: vi.fn(),
  },
}));
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([]);
  vi.mocked(routingUpstreamsApi.save).mockImplementation(async (value) => ({
    ...value,
    id: "relay",
    revision: 1,
  }));
  vi.mocked(fetchModelsForConfig).mockResolvedValue([
    { id: "first", ownedBy: "" },
    { id: "second", ownedBy: "" },
  ]);
});
afterEach(async () => {
  cleanup();
  // Radix restores focus in a timer. Let it finish in the same jsdom document.
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
});

async function custom() {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
  const add = await screen.findByRole("button", { name: "modelProviders.add" });
  await waitFor(() => expect(add).toBeEnabled());
  fireEvent.click(add);
  expect(
    screen.queryByRole("button", { name: "providerFlow.import" }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "providerFlow.custom" }));
  const dialog = screen.getByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("modelProviders.name"), {
    target: { value: "Relay" },
  });
  fireEvent.change(within(dialog).getByLabelText("modelProviders.baseUrl"), {
    target: { value: "https://relay.example/v1" },
  });
  fireEvent.change(within(dialog).getByLabelText("modelProviders.apiKey"), {
    target: { value: "synthetic-key" },
  });
  return dialog;
}
it("starts a custom provider with only optional model ids, no UUID, switch, modes or JSON", async () => {
  const dialog = await custom();
  expect(within(dialog).queryByLabelText("ID")).not.toBeInTheDocument();
  expect(within(dialog).queryByRole("switch")).not.toBeInTheDocument();
  expect(
    within(dialog).queryByRole("radiogroup", {
      name: "providerEditor.modelView",
    }),
  ).not.toBeInTheDocument();
  expect(within(dialog).queryByRole("searchbox")).not.toBeInTheDocument();
  expect(
    within(dialog).queryByRole("textbox", { name: "modelProviders.models" }),
  ).not.toBeInTheDocument();
  expect(
    within(dialog).getByRole("radio", { name: "Chat Completions" }),
  ).toHaveAttribute("aria-checked", "true");
});
it("requires a unique name before creating a provider", async () => {
  vi.mocked(routingUpstreamsApi.list).mockResolvedValue([
    {
      schemaVersion: 1,
      id: "existing-relay",
      name: "  relay  ",
      revision: 1,
      hasApiKey: false,
      source: {
        kind: "api",
        baseUrl: "https://relay.example/v1",
        apiFormat: "openai_chat",
        models: [],
        catalog: [],
        mode: "vendor",
        groupsOnly: false,
      },
    },
  ]);
  const dialog = await custom();
  expect(within(dialog).getByLabelText("modelProviders.name")).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  expect(
    within(dialog).getByRole("button", { name: "common.add" }),
  ).toBeDisabled();
  fireEvent.submit(dialog.querySelector("form")!);
  expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
});
it("fetches one candidate list and preselects it in the unsaved draft", async () => {
  const dialog = await custom();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "providerForm.fetchModels" }),
  );
  expect(
    await within(dialog).findByRole("button", { name: "first" }),
  ).toHaveAttribute("aria-pressed", "true");
  const second = within(dialog).getByRole("button", { name: "second" });
  expect(second).toHaveAttribute("aria-pressed", "true");
  expect(routingUpstreamsApi.save).not.toHaveBeenCalled();
  fireEvent.click(second);
  fireEvent.submit(dialog.querySelector("form")!);
  await waitFor(() => expect(routingUpstreamsApi.save).toHaveBeenCalled());
  expect(
    vi.mocked(routingUpstreamsApi.save).mock.calls[0][0].source,
  ).toMatchObject({
    mode: "picked",
    models: [{ model: "first" }],
    catalog: [{ model: "first" }, { model: "second" }],
  });
  expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
});
it("saves an empty selection without a blocking discovery and follows the vendor list", async () => {
  const dialog = await custom();
  fireEvent.submit(dialog.querySelector("form")!);
  await waitFor(() => expect(routingUpstreamsApi.save).toHaveBeenCalled());
  expect(
    vi.mocked(routingUpstreamsApi.save).mock.calls[0][0].source,
  ).toMatchObject({ mode: "vendor", models: [] });
  expect(fetchModelsForConfig).not.toHaveBeenCalled();
});

it("preselects and saves an image-only catalog as an explicit selection", async () => {
  vi.mocked(fetchModelsForConfig).mockResolvedValue([
    { id: "gpt-image-2", ownedBy: "" },
  ]);
  const dialog = await custom();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "providerForm.fetchModels" }),
  );
  expect(
    await within(dialog).findByRole("button", { name: "gpt-image-2" }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(
    within(dialog).getByRole("checkbox", { name: "providerEditor.selectAll" }),
  ).toBeDisabled();
  fireEvent.submit(dialog.querySelector("form")!);
  await waitFor(() => expect(routingUpstreamsApi.save).toHaveBeenCalled());
  expect(
    vi.mocked(routingUpstreamsApi.save).mock.calls[0][0].source,
  ).toMatchObject({ mode: "picked", models: [{ model: "gpt-image-2" }] });
});
it("keeps manual model entry removed and selects exact discovered model IDs without duplicates", async () => {
  const dialog = await custom();
  expect(
    within(dialog).queryByLabelText("providerEditor.addModelId"),
  ).toBeNull();
  expect(
    within(dialog).queryByRole("button", { name: "providerEditor.addModel" }),
  ).toBeNull();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "providerForm.fetchModels" }),
  );
  const second = await within(dialog).findByRole("button", { name: "second" });
  fireEvent.click(second);
  expect(second).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(
    within(dialog).getByRole("checkbox", { name: "providerEditor.selectAll" }),
  );
  fireEvent.submit(dialog.querySelector("form")!);
  await waitFor(() => expect(routingUpstreamsApi.save).toHaveBeenCalled());
  expect(
    vi.mocked(routingUpstreamsApi.save).mock.calls[0][0].source,
  ).toMatchObject({
    mode: "picked",
    models: [{ model: "first" }, { model: "second" }],
  });
});
