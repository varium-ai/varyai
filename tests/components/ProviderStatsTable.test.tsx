import { render, screen, within } from "@testing-library/react";
import i18n from "i18next";
import { expect, it, vi } from "vitest";
import { ProviderStatsTable } from "@/components/usage/ProviderStatsTable";
import en from "@/i18n/locales/en.json";

vi.mock("@/lib/query/usage", () => ({
  useProviderStats: () => ({
    isLoading: false,
    data: ["_codex_session", "saved"].map((providerId) => ({
      providerId,
      providerName: "Codex (Session)",
      requestCount: 7,
      totalTokens: 150,
      totalCost: "0.5",
      successRate: 100,
      avgLatencyMs: 20,
    })),
  }),
}));

it("shows a plain Codex source name while retaining usage and saved supplier names", () => {
  i18n.addResourceBundle("en", "translation", { usage: en.usage }, true, true);
  void i18n.changeLanguage("en");
  render(
    <ProviderStatsTable range={{ preset: "today" }} refreshIntervalMs={0} />,
  );
  expect(
    screen.getByRole("columnheader", { name: "Provider / source" }),
  ).toBeVisible();
  expect(screen.getByText("Codex")).toBeVisible();
  expect(screen.queryByText("Provider unattributed")).not.toBeInTheDocument();
  expect(screen.getByText("Codex (Session)")).toBeVisible();
  const row = screen.getByText("Codex").closest("tr");
  expect(row).not.toBeNull();
  expect(within(row!).getByText("7")).toBeVisible();
  expect(within(row!).getByText("$0.5000")).toBeVisible();
});
