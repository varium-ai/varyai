import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { RoutingUpstreamsPanel } from "@/components/providers/RoutingUpstreamsPanel";
import { routingUpstreamsApi } from "@/lib/api/routingUpstreams";

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
function mount() {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <RoutingUpstreamsPanel />
    </QueryClientProvider>,
  );
}
it("unrolls the vendor sheet before opening a provider editor", async () => {
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
it("requires confirmation before discarding edits and returns to the sheet", async () => {
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
it("clears the vendor search with Escape before folding the sheet", async () => {
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
});
