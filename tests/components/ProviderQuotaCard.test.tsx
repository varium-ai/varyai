import { render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { ProviderQuotaCard } from "@/components/providers/ProviderQuotaCard";

const tier = {
  name: "Gemini 4 Argon · seven_day",
  utilization: 0,
  resetsAt: "2030-01-01T12:00:00Z",
};

it("names an unused window while retaining its full model and reset in the ring tooltip", () => {
  render(
    <ProviderQuotaCard
      name="Synthetic account"
      items={[{ tier, label: "Gemini 4 Argon 7d" }]}
      status={null}
      fetching={false}
      loading={false}
      refresh={vi.fn()}
      card={{ remaining: true, toggle: vi.fn() }}
    />,
  );
  expect(screen.getByText("quotaPanel.unused")).toBeVisible();
  expect(screen.getByText("Gemini 4")).toBeVisible();
  expect(
    screen.getByRole("meter", { name: "Gemini 4 Argon 7d" }),
  ).toHaveAttribute("aria-valuenow", "100");
  expect(
    screen.getByRole("button", { name: /Gemini 4 Argon 7d/ }),
  ).toHaveAttribute(
    "title",
    expect.stringContaining("Gemini 4 Argon 7d\ncodexProvider.resets"),
  );
});

it("keeps a stale status visible even when the last reading was unused", () => {
  render(
    <ProviderQuotaCard
      name="Synthetic account"
      items={[{ tier, label: "Gemini 4 Argon 7d" }]}
      status="codexProvider.quotaStale"
      fetching={false}
      loading={false}
      refresh={vi.fn()}
      card={{ remaining: true, toggle: vi.fn() }}
    />,
  );
  expect(screen.getByText("codexProvider.quotaStale")).toBeVisible();
  expect(screen.queryByText("quotaPanel.unused")).not.toBeInTheDocument();
});
