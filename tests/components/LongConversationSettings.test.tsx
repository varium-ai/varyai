import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LongConversationSettings } from "@/components/agents/LongConversationSettings";
import { intentRoutingApi } from "@/lib/api/intentRouting";

vi.mock("@/lib/api/intentRouting", () => ({
  intentRoutingApi: {
    contextSettings: vi.fn(),
    setContextSettings: vi.fn(),
  },
}));

function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <LongConversationSettings />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(intentRoutingApi.contextSettings).mockResolvedValue({
    fullWindow: false,
    compactAt: 350000,
  });
  vi.mocked(intentRoutingApi.setContextSettings).mockResolvedValue();
});

describe("Long conversations", () => {
  it("switches between compaction and full window while retaining the saved threshold", async () => {
    mount();
    const full = await screen.findByRole("radio", {
      name: "longConversations.fullWindow",
    });
    await waitFor(() => expect(full).toBeEnabled());
    fireEvent.click(full);
    await waitFor(() =>
      expect(intentRoutingApi.setContextSettings).toHaveBeenCalledWith({
        fullWindow: true,
        compactAt: 350000,
      }),
    );
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("radio", { name: "longConversations.compact" }),
    );
    await waitFor(() =>
      expect(intentRoutingApi.setContextSettings).toHaveBeenLastCalledWith({
        fullWindow: false,
        compactAt: 350000,
      }),
    );
    expect(await screen.findByRole("textbox")).toHaveValue("350k");
    expect(
      screen.queryByRole("button", { name: "common.save" }),
    ).not.toBeInTheDocument();
  });

  it("accepts k/m token sizes and an empty default, but never persists invalid input", async () => {
    mount();
    const input = await screen.findByRole("textbox", {
      name: "longConversations.threshold",
    });
    await waitFor(() => expect(input).toHaveValue("350k"));
    fireEvent.change(input, { target: { value: "1m" } });
    fireEvent.blur(input);
    await waitFor(() =>
      expect(intentRoutingApi.setContextSettings).toHaveBeenCalledWith({
        fullWindow: false,
        compactAt: 1000000,
      }),
    );
    fireEvent.change(input, { target: { value: "invalid" } });
    fireEvent.blur(input);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "longConversations.invalid",
    );
    expect(intentRoutingApi.setContextSettings).toHaveBeenCalledTimes(1);
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() =>
      expect(intentRoutingApi.setContextSettings).toHaveBeenLastCalledWith({
        fullWindow: false,
      }),
    );
  });

  it("keeps the saved mode after a failure and never renders raw errors", async () => {
    vi.mocked(intentRoutingApi.setContextSettings).mockRejectedValue(
      "synthetic-private-error",
    );
    mount();
    const full = await screen.findByRole("radio", {
      name: "longConversations.fullWindow",
    });
    await waitFor(() => expect(full).toBeEnabled());
    fireEvent.click(full);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "longConversations.failed",
    );
    expect(full).not.toBeChecked();
    expect(
      screen.queryByText("synthetic-private-error"),
    ).not.toBeInTheDocument();
  });
});
