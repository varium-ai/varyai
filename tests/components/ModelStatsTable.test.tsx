import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ModelStatsTable } from "@/components/usage/ModelStatsTable";
import en from "@/i18n/locales/en.json";

const id = `model-${"c".repeat(64)}`;
vi.mock("@/lib/query/usage", () => ({
  useModelStats: () => ({
    data: [
      {
        model: `vary-provider/model-${"c".repeat(64)}`,
        requestCount: 2,
        totalTokens: 30,
        totalCost: "0.5",
        avgCostPerRequest: "0.25",
      },
    ],
    isLoading: false,
  }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, args?: { id?: string }) =>
      key === "usage.unknownRoutingModelOption"
        ? en.usage.unknownRoutingModelOption.replace("{{id}}", args?.id ?? "")
        : key,
  }),
}));

describe("ModelStatsTable model names", () => {
  it("uses the same supplier/model label as the picker without changing totals", () => {
    render(
      <ModelStatsTable
        range={{ preset: "today" }}
        refreshIntervalMs={0}
        modelNames={{ [id]: "Synthetic Model · Synthetic supplier" }}
      />,
    );
    expect(
      screen.getByText("Synthetic Model · Synthetic supplier"),
    ).toBeVisible();
    expect(screen.getByText("30")).toBeVisible();
    expect(screen.queryByText(`vary-provider/${id}`)).not.toBeInTheDocument();
  });
  it("clearly labels unrecoverable history instead of exposing a full internal ID", () => {
    render(
      <ModelStatsTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );
    expect(
      screen.getByText("Unknown historical model · cccccccccccc"),
    ).toBeVisible();
    expect(screen.queryByText(`vary-provider/${id}`)).not.toBeInTheDocument();
  });
});
