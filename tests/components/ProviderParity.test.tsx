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
it("fetches one candidate list without automatically picking or saving models", async () => {
  const dialog = await custom();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "providerForm.fetchModels" }),
  );
  const first = await within(dialog).findByRole("button", { name: "first" });
  expect(first).toHaveAttribute("aria-pressed", "false");
  expect(
    within(dialog).getByRole("button", { name: "second" }),
  ).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(first);
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

it("keeps image-only picks from switching a new provider's chat list to picked mode", async () => {
  const dialog = await custom();
  fireEvent.change(within(dialog).getByLabelText("providerEditor.addModelId"), {
    target: { value: "gpt-image-2" },
  });
  fireEvent.submit(dialog.querySelector("form")!);
  await waitFor(() => expect(routingUpstreamsApi.save).toHaveBeenCalled());
  expect(
    vi.mocked(routingUpstreamsApi.save).mock.calls[0][0].source,
  ).toMatchObject({
    mode: "vendor",
    models: [{ model: "gpt-image-2" }],
  });
});
it("accepts comma-separated manual model IDs", async () => {
  const dialog = await custom();
  const ids = within(dialog).getByLabelText("providerEditor.addModelId");
  fireEvent.change(ids, { target: { value: "alpha, beta, alpha" } });
  fireEvent.submit(dialog.querySelector("form")!);
  await waitFor(() => expect(routingUpstreamsApi.save).toHaveBeenCalled());
  expect(
    vi.mocked(routingUpstreamsApi.save).mock.calls[0][0].source,
  ).toMatchObject({
    mode: "picked",
    models: [{ model: "alpha" }, { model: "beta" }],
  });
});
