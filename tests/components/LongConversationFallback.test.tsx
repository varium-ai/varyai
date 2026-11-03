import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { LongConversationFallback } from "@/components/settings/LongConversationFallback";
import { proxyApi } from "@/lib/api/proxy";
import { intentRoutingApi } from "@/lib/api/intentRouting";

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: { getAgentInstallations: vi.fn() },
}));
vi.mock("@/lib/api/intentRouting", () => ({
  intentRoutingApi: { contextSettings: vi.fn(), setContextSettings: vi.fn() },
}));

function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <LongConversationFallback />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(intentRoutingApi.contextSettings).mockResolvedValue({
    fullWindow: false,
  });
});

it("offers the shared control in Settings when none of its tools is installed", async () => {
  vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
    pi: true,
  } as never);
  mount();
  expect(
    await screen.findByRole("radiogroup", { name: "longConversations.title" }),
  ).toBeVisible();
  expect(intentRoutingApi.setContextSettings).not.toHaveBeenCalled();
});

it.each(["codex", "claude", "grokbuild"])(
  "leaves the control in %s tool settings when installed",
  async (app) => {
    vi.mocked(proxyApi.getAgentInstallations).mockResolvedValue({
      [app]: true,
    } as never);
    mount();
    await waitFor(() =>
      expect(proxyApi.getAgentInstallations).toHaveBeenCalledOnce(),
    );
    expect(screen.queryByRole("radiogroup")).not.toBeInTheDocument();
    expect(intentRoutingApi.contextSettings).not.toHaveBeenCalled();
  },
);

it("does not expose fallback settings based on a failed installation read", async () => {
  vi.mocked(proxyApi.getAgentInstallations).mockRejectedValue(
    "synthetic-private-error",
  );
  mount();
  await waitFor(() =>
    expect(proxyApi.getAgentInstallations).toHaveBeenCalledOnce(),
  );
  expect(screen.queryByRole("radiogroup")).not.toBeInTheDocument();
  expect(intentRoutingApi.contextSettings).not.toHaveBeenCalled();
});
