import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { expect, it, vi } from "vitest";
import { RoutingRequestHistory } from "@/components/routing/RoutingRequestHistory";
vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    getRequestLogs: vi.fn(async () => ({
      data: [
        {
          requestId: "fixture",
          model: "actual-model",
          requestModel: "group/shared",
          providerName: "Synthetic",
          appType: "claude",
          statusCode: 200,
          latencyMs: 120,
          createdAt: 1700000000,
          inputTokens: 10,
          outputTokens: 20,
          errorMessage: "synthetic-private-error",
        },
      ],
      total: 1,
    })),
  },
}));
it("opens completed request metadata without exposing upstream errors or inputs", async () => {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RoutingRequestHistory />
    </QueryClientProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: /group\/shared/ }));
  expect(screen.getByText("actual-model")).toBeInTheDocument();
  expect(screen.getByText("Synthetic")).toBeInTheDocument();
  expect(screen.queryByText("synthetic-private-error")).not.toBeInTheDocument();
});
