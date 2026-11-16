import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { quotaPanelApi } from "@/lib/api/quotaPanel";
import { QuotaSparkline } from "@/components/providers/QuotaSparkline";

vi.mock("@/lib/api/quotaPanel", () => ({
  quotaPanelApi: { history: vi.fn(), note: vi.fn() },
}));
const now = Date.parse("2030-01-01T12:00:00Z");
const reset = now / 1000 + 9000;
const point = {
  at: now / 1000 - 7200,
  remaining: 90,
  resetsAt: reset - 5,
  span: 18000,
};
function mount() {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <QuotaSparkline
        scope="synthetic:pool"
        sampledAt={now}
        tier={{
          name: "Gemini · five_hour",
          utilization: 40,
          resetsAt: new Date(reset * 1000).toISOString(),
        }}
        label="Gemini 5h"
      />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.mocked(quotaPanelApi.history).mockResolvedValue([point]);
  vi.mocked(quotaPanelApi.note).mockResolvedValue([point]);
});
it("draws a pooled window with a solid history line and both pace bands", async () => {
  mount();
  const chart = await screen.findByRole("img");
  await waitFor(() =>
    expect(chart.querySelector(".tray-quota-history")).toHaveAttribute(
      "d",
      "M20.00,3.00 L100.00,12.00",
    ),
  );
  expect(chart.querySelector(".tray-quota-gap-ahead")).toBeInTheDocument();
  expect(chart.querySelector(".tray-quota-gap-behind")).toBeInTheDocument();
  expect(quotaPanelApi.history).toHaveBeenCalledWith(
    "synthetic:pool:Gemini · five_hour",
  );
});
it("keeps the latest point on a history failure without fabricating a line or shading", async () => {
  vi.mocked(quotaPanelApi.history).mockRejectedValue(
    new Error("Synthetic private history failure"),
  );
  vi.mocked(quotaPanelApi.note).mockRejectedValue(
    new Error("Synthetic private storage failure"),
  );
  mount();
  const chart = await screen.findByRole("img");
  await waitFor(() => expect(quotaPanelApi.note).toHaveBeenCalled());
  expect(chart.querySelector(".tray-quota-history")).toHaveAttribute(
    "d",
    "M100.00,12.00h0.01",
  );
  expect(chart.querySelector(".tray-quota-gap-ahead")).not.toBeInTheDocument();
  expect(screen.queryByText(/Synthetic private/)).not.toBeInTheDocument();
});
