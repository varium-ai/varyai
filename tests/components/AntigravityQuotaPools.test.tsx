import { render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import i18n from "i18next";
import { SubscriptionAccountQuota } from "@/components/providers/SubscriptionAccountQuota";
import { nativeSubscriptionApi } from "@/lib/api/nativeSubscription";
import fixture from "../fixtures/antigravity-quota-pools.json";
vi.mock("@/lib/api/nativeSubscription", () => ({
  nativeSubscriptionApi: { quota: vi.fn() },
}));
beforeEach(() => {
  vi.clearAllMocks();
  i18n.addResourceBundle(
    "zh",
    "translation",
    { codexProvider: { remaining: "{{n}}% left" } },
    true,
    true,
  );
  vi.mocked(nativeSubscriptionApi.quota).mockResolvedValue({
    credentialStatus: "valid",
    success: true,
    tiers: fixture.expectedTiers,
  } as never);
});
it.each([true, false])(
  "keeps all reported and independent Antigravity pools (compact=%s)",
  async (compact) => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SubscriptionAccountQuota
          provider="antigravity"
          id="fixture-account"
          login="Fixture"
          ready
          disabled={false}
          compact={compact}
        />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("96.8% left")).toBeVisible();
    for (const label of [
      "Gemini 5h",
      "Gemini 7d",
      "Claude 5h",
      "Claude & GPT 7d",
      "Gemini 4 Argon 7d",
      "GPT-OSS 5h",
    ]) {
      expect(screen.getByText(label, { exact: false })).toBeVisible();
    }
    expect(screen.getAllByText("0% left")).toHaveLength(4);
    expect(nativeSubscriptionApi.quota).toHaveBeenCalledExactlyOnceWith(
      "antigravity",
      "fixture-account",
    );
  },
);
it("normalizes cached upstream bucket labels and retains decimal remaining values in both views", async () => {
  vi.mocked(nativeSubscriptionApi.quota).mockResolvedValue({
    credentialStatus: "valid",
    success: true,
    tiers: [
      {
        name: "Gemini Models · Gemini Weekly Limit Remaining",
        utilization: 3.2,
        resetsAt: null,
      },
    ],
  } as never);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <section aria-label="List">
        <SubscriptionAccountQuota
          provider="antigravity"
          id="fixture"
          login="Fixture"
          ready
          disabled={false}
          compact
        />
      </section>
      <section aria-label="Form">
        <SubscriptionAccountQuota
          provider="antigravity"
          id="fixture"
          login="Fixture"
          ready
          disabled={false}
        />
      </section>
    </QueryClientProvider>,
  );
  for (const name of ["List", "Form"]) {
    const region = within(screen.getByRole("region", { name }));
    expect(await region.findByText(/Gemini 7d/)).toBeVisible();
    expect(region.getByText("96.8% left")).toBeVisible();
    expect(
      region.queryByText(/Weekly Limit Remaining/),
    ).not.toBeInTheDocument();
  }
  expect(nativeSubscriptionApi.quota).toHaveBeenCalledTimes(1);
});
