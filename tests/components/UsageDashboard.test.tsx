import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ComponentProps } from "react";
import i18n from "i18next";
import en from "@/i18n/locales/en.json";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDashboard } from "@/components/usage/UsageDashboard";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useModelStatsMock = vi.hoisted(() => vi.fn());
const usageHeroMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (...args: Parameters<typeof i18n.t>) => i18n.t(...args),
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

vi.mock("@/hooks/useUsageEventBridge", () => ({
  useUsageEventBridge: () => {},
}));

vi.mock("@/lib/query/usage", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/query/usage")>(
      "@/lib/query/usage",
    );
  return {
    ...actual,
    useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
    useModelStats: (...args: unknown[]) => useModelStatsMock(...args),
  };
});

vi.mock("@/components/usage/UsageHero", () => ({
  UsageHero: (props: unknown) => {
    usageHeroMock(props);
    return <div data-testid="usage-hero" />;
  },
}));

vi.mock("@/components/usage/UsageTrendChart", () => ({
  UsageTrendChart: () => <div data-testid="usage-trend" />,
}));

vi.mock("@/components/usage/RequestLogTable", () => ({
  RequestLogTable: () => <div data-testid="request-log-table" />,
}));

vi.mock("@/components/usage/ProviderStatsTable", () => ({
  ProviderStatsTable: () => <div data-testid="provider-stats-table" />,
}));

vi.mock("@/components/usage/ModelStatsTable", () => ({
  ModelStatsTable: () => <div data-testid="model-stats-table" />,
}));

vi.mock("@/components/usage/PricingConfigPanel", () => ({
  PricingConfigPanel: () => <div data-testid="pricing-config-panel" />,
}));

vi.mock("@/components/usage/UsageDateRangePicker", () => ({
  UsageDateRangePicker: () => <button type="button">date-range</button>,
}));

vi.mock("@/components/ui/select", async () => {
  const { createContext, useContext } = await import("react");
  const Selection = createContext<(value: string) => void>(() => {});
  return {
    Select: ({ value, onValueChange, children }: any) => (
      <Selection.Provider value={onValueChange}>
        <div data-testid={`select-${value}`}>{children}</div>
      </Selection.Provider>
    ),
    SelectTrigger: ({ children, ...props }: any) => (
      <button type="button" {...props}>
        {children}
      </button>
    ),
    SelectValue: () => null,
    SelectContent: ({ children }: any) => <div>{children}</div>,
    SelectItem: ({ children, value, ...props }: any) => {
      const select = useContext(Selection);
      return (
        <button type="button" onClick={() => select(value)} {...props}>
          {children}
        </button>
      );
    },
    SelectGroup: ({ children }: any) => <div>{children}</div>,
    SelectLabel: ({ children }: any) => <div>{children}</div>,
    SelectSeparator: () => <hr />,
  };
});

const renderDashboard = (props: ComponentProps<typeof UsageDashboard> = {}) => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <UsageDashboard {...props} />
    </QueryClientProvider>,
  );
};

describe("UsageDashboard", () => {
  beforeEach(() => {
    i18n.addResourceBundle(
      "en",
      "translation",
      { usage: en.usage },
      true,
      true,
    );
    void i18n.changeLanguage("en");
    useProviderStatsMock.mockReset();
    useModelStatsMock.mockReset();
    usageHeroMock.mockReset();
    useProviderStatsMock.mockReturnValue({ data: [] });
    useModelStatsMock.mockReturnValue({ data: [] });
  });

  it("initializes the today view with the requested app", () => {
    renderDashboard({ initialAppType: "codex" });
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ appType: "codex", range: { preset: "today" } }),
    );
  });

  it("uses the saved refresh interval when mounted", () => {
    renderDashboard({ refreshIntervalMs: 5000 });

    expect(screen.getByTestId("select-5000")).toBeInTheDocument();
  });

  it("filters usage queries to Pi", async () => {
    renderDashboard();

    fireEvent.click(screen.getByRole("button", { name: "Pi" }));

    await waitFor(() =>
      expect(useProviderStatsMock).toHaveBeenLastCalledWith(
        expect.anything(),
        { appType: "pi" },
        expect.anything(),
      ),
    );
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      expect.anything(),
      { appType: "pi", providerName: undefined },
      expect.anything(),
    );
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ appType: "pi" }),
    );
  });

  it("persists refresh interval changes", async () => {
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(true);
    renderDashboard({ onRefreshIntervalChange });

    fireEvent.click(
      within(screen.getByTestId("select-30000")).getByRole("button", {
        name: "5s",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    expect(screen.getByTestId("select-5000")).toBeInTheDocument();
  });

  it("rolls back optimistic interval changes when persistence fails", async () => {
    const onRefreshIntervalChange = vi.fn().mockResolvedValue(false);
    renderDashboard({ onRefreshIntervalChange });

    fireEvent.click(
      within(screen.getByTestId("select-30000")).getByRole("button", {
        name: "5s",
      }),
    );

    await waitFor(() =>
      expect(onRefreshIntervalChange).toHaveBeenCalledWith(5000),
    );
    await waitFor(() =>
      expect(screen.getByTestId("select-30000")).toBeInTheDocument(),
    );
  });

  it("lists Codex alongside suppliers without a session group and retains its raw filter value", () => {
    useProviderStatsMock.mockReturnValue({
      data: [
        { providerId: "_codex_session", providerName: "Codex (Session)" },
        { providerId: "saved", providerName: "Synthetic supplier" },
      ],
    });
    const view = renderDashboard();
    expect(screen.queryByText("Session imports")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Providers / other sources"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/not linked to a specific provider/),
    ).not.toBeInTheDocument();
    const sources = within(screen.getAllByTestId("select-all")[0]);
    expect(
      sources.getByRole("button", { name: "Synthetic supplier" }),
    ).toBeVisible();
    fireEvent.click(sources.getByRole("button", { name: "Codex" }));
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({
        providerName: "Codex (Session)",
      }),
    );
    expect(useModelStatsMock).toHaveBeenLastCalledWith(
      expect.anything(),
      { appType: "all", providerName: "Codex (Session)" },
      expect.anything(),
    );

    useProviderStatsMock.mockReturnValue({ data: [] });
    view.rerender(
      <QueryClientProvider client={new QueryClient()}>
        <UsageDashboard />
      </QueryClientProvider>,
    );
    expect(
      within(screen.getByTestId("select-v:Codex (Session)")).getByRole(
        "button",
        { name: "Codex" },
      ),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "All Sources" }));
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ providerName: undefined }),
    );
  });

  it("does not relabel a saved supplier or a mixed name filter as a session import", () => {
    useProviderStatsMock.mockReturnValue({
      data: [
        { providerId: "saved", providerName: "Codex (Session)" },
        { providerId: "_codex_session", providerName: "Codex (Session)" },
        { providerId: "saved-claude", providerName: "Claude (Session)" },
        { providerId: "saved-all", providerName: "all" },
      ],
    });
    renderDashboard();
    expect(
      screen.getAllByRole("button", { name: "Codex (Session)" }),
    ).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "Claude (Session)" }),
    ).toBeVisible();
    expect(
      screen.queryByText("Codex · Session import"),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "all" }));
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ providerName: "all" }),
    );
  });
});

it("offers the observed request and response model IDs alongside billing IDs", () => {
  useModelStatsMock.mockReturnValue({
    data: [
      {
        model: "billing-model",
        aliases: ["configured-model", "response-model"],
        requestCount: 1,
        totalTokens: 10,
        totalCost: "0.5",
        avgCostPerRequest: "0.5",
      },
    ],
  });
  renderDashboard();
  for (const model of ["billing-model", "configured-model", "response-model"])
    expect(screen.getByText(model)).toBeVisible();
  expect(screen.getByText(en.usage.modelFilterScope)).toBeVisible();
});
