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
import { APP_IDS } from "@/config/appConfig";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
const useModelStatsMock = vi.hoisted(() => vi.fn());
const useUsageModelLabelsMock = vi.hoisted(() => vi.fn());
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
    useUsageModelLabels: (...args: unknown[]) =>
      useUsageModelLabelsMock(...args),
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
      { usage: en.usage, settings: en.settings },
      true,
      true,
    );
    void i18n.changeLanguage("en");
    useProviderStatsMock.mockReset();
    useModelStatsMock.mockReset();
    useUsageModelLabelsMock.mockReset();
    useUsageModelLabelsMock.mockReturnValue({ data: [] });
    usageHeroMock.mockReset();
    useProviderStatsMock.mockReturnValue({ data: [] });
    useModelStatsMock.mockReturnValue({ data: [] });
  });

  it.each(
    ["all", ...APP_IDS].flatMap((appType) =>
      [true, false].map((autoSync) => ({ appType, autoSync })),
    ),
  )(
    "omits pricing and Codex maintenance for $appType with auto sync $autoSync",
    ({ appType, autoSync }) => {
      const onSessionAutoSyncEnabledChange = vi.fn();
      renderDashboard({
        initialAppType: appType,
        sessionAutoSyncEnabled: autoSync,
        onSessionAutoSyncEnabledChange,
      });

      expect(
        screen.queryByRole("button", {
          name: new RegExp(en.settings.advanced.pricing.title),
        }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", {
          name: new RegExp(en.usage.rebuildCodex.title),
        }),
      ).not.toBeInTheDocument();
      expect(screen.getByTestId("request-log-table")).toBeInTheDocument();
      const syncButton = screen.queryByRole("button", {
        name: en.usage.sessionSync.syncNow,
      });
      if (autoSync) {
        expect(syncButton).not.toBeInTheDocument();
      } else {
        expect(syncButton).toBeInTheDocument();
      }
      const toggle = screen.getByRole("switch", {
        name: en.usage.sessionSync.title,
      });
      expect(toggle).toHaveAttribute("aria-checked", String(autoSync));
      fireEvent.click(toggle);
      expect(onSessionAutoSyncEnabledChange).toHaveBeenCalledWith(!autoSync);
    },
  );

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

  it("hides unresolved bare and qualified routing IDs but retains readable models", () => {
    const bare = `model-${"1".repeat(64)}`;
    const qualified = `vary-provider/model-${"2".repeat(64)}`;
    useModelStatsMock.mockReturnValue({
      data: [
        { model: bare },
        { model: qualified },
        { model: "readable-model" },
      ],
    });
    renderDashboard();
    expect(
      screen.getByRole("button", { name: "readable-model" }),
    ).toBeVisible();
    expect(
      screen.queryByText(/Unknown historical model/),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(bare)).not.toBeInTheDocument();
    expect(screen.queryByText(qualified)).not.toBeInTheDocument();
  });

  it("omits malformed session model names without hiding full Codex model names", () => {
    useModelStatsMock.mockReturnValue({
      data: [
        { model: "-codex" },
        { model: "unknown" },
        { model: "gpt-synthetic-codex" },
        { model: "codex-auto-review" },
      ],
    });
    renderDashboard();
    expect(
      screen.queryByRole("button", { name: "-codex" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "unknown" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "gpt-synthetic-codex" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "codex-auto-review" }),
    ).toBeVisible();
  });

  it("resolves qualified session IDs with supplier names and selects the shared identity", () => {
    const id = `model-${"9".repeat(64)}`;
    useModelStatsMock.mockReturnValue({
      data: [{ model: `vary-provider/${id}`, aliases: [id] }],
    });
    useUsageModelLabelsMock.mockReturnValue({
      data: [
        {
          id,
          model: "synthetic-model",
          displayName: "Synthetic Model",
          providerName: "Synthetic supplier",
        },
      ],
    });
    renderDashboard();
    expect(screen.queryByText(`vary-provider/${id}`)).not.toBeInTheDocument();
    expect(
      screen.getAllByRole("button", {
        name: "Synthetic Model · Synthetic supplier",
      }),
    ).toHaveLength(1);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Synthetic Model · Synthetic supplier",
      }),
    );
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ model: id }),
    );
    expect(useUsageModelLabelsMock).toHaveBeenLastCalledWith(
      expect.arrayContaining([id, `vary-provider/${id}`]),
      expect.anything(),
    );
  });

  it("labels internal routing aliases with billing models while retaining raw filter IDs", () => {
    const routingId = `model-${"a".repeat(64)}`;
    useModelStatsMock.mockReturnValue({
      data: [
        { model: "synthetic-model", aliases: [routingId, "response-model"] },
      ],
    });
    const view = renderDashboard();

    expect(screen.queryByText(routingId)).not.toBeInTheDocument();
    const option = screen.getByRole("button", {
      name: "synthetic-model (routing ID)",
    });
    expect(option).toHaveAttribute("title", "synthetic-model (routing ID)");
    fireEvent.click(option);
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ model: routingId }),
    );
    expect(screen.getByTestId(`select-v:${routingId}`)).toBeInTheDocument();
    expect(
      within(screen.getByTestId(`select-v:${routingId}`)).getByRole("button", {
        description: en.usage.modelFilterScope,
      }),
    ).toHaveAttribute("title", "synthetic-model (routing ID)");

    // An unresolved selection must not remain as an invisible active filter.
    useModelStatsMock.mockReturnValue({ data: [] });
    view.rerender(
      <QueryClientProvider client={new QueryClient()}>
        <UsageDashboard />
      </QueryClientProvider>,
    );
    expect(
      screen.queryByTestId(`select-v:${routingId}`),
    ).not.toBeInTheDocument();
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ model: undefined }),
    );
    expect(
      screen.queryByText(/Unknown historical model/),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(routingId)).not.toBeInTheDocument();
  });

  it("collects all billing models for a routing ID instead of guessing one target", () => {
    const routingId = `model-${"b".repeat(64)}`;
    useModelStatsMock.mockReturnValue({
      data: [
        { model: "model-beta", aliases: [routingId] },
        { model: "model-alpha", aliases: [routingId] },
        { model: "model-beta", aliases: [routingId] },
      ],
    });
    renderDashboard();
    expect(
      screen.getAllByText("model-alpha, model-beta (routing ID)"),
    ).toHaveLength(1);
    expect(screen.queryByText(routingId)).not.toBeInTheDocument();
  });

  it("distinguishes routing IDs that share the same readable model label", () => {
    const firstId = `model-${"e".repeat(64)}`;
    const secondId = `model-${"f".repeat(64)}`;
    useModelStatsMock.mockReturnValue({
      data: [{ model: "shared-model", aliases: [firstId, secondId] }],
    });
    renderDashboard();
    expect(
      screen.getByText("shared-model (routing ID) · eeeeeeeeeeee"),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: "shared-model (routing ID) · ffffffffffff",
      }),
    );
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ model: secondId }),
    );
  });

  it("labels historical routing IDs using known aliases or a short fallback", () => {
    const knownId = `model-${"c".repeat(64)}`;
    const unknownId = `model-${"d".repeat(64)}`;
    useModelStatsMock.mockReturnValue({
      data: [
        { model: knownId, aliases: [knownId, "synthetic-response"] },
        { model: unknownId, aliases: [unknownId] },
        { model: "model-custom", aliases: ["model-abcdef", "all"] },
      ],
    });
    renderDashboard();
    expect(screen.getByText("synthetic-response (routing ID)")).toBeVisible();
    expect(
      screen.queryByText("Unknown historical model · dddddddddddd"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(knownId)).not.toBeInTheDocument();
    expect(screen.queryByText(unknownId)).not.toBeInTheDocument();
    expect(screen.getByText("model-custom")).toBeVisible();
    expect(screen.getByText("model-abcdef")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "all" }));
    expect(usageHeroMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ model: "all" }),
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

it("offers one filter for publisher-qualified DeepSeek and GLM IDs without merging versions or custom namespaces", () => {
  useModelStatsMock.mockReturnValue({
    data: [
      {
        model: "deepseek-v4.1-flash",
        aliases: ["deepseek-ai/DeepSeek-V4.1-Flash"],
      },
      { model: "deepseek-ai/DeepSeek-V4.1-Flash", aliases: [] },
      { model: "zai-org/GLM-5.2-FP8", aliases: ["glm-5.2-fp8"] },
      { model: "deepseek-v4.1", aliases: [] },
      { model: "custom/DeepSeek-V4.1-Flash", aliases: [] },
    ],
  });
  renderDashboard();
  expect(screen.getAllByText("deepseek-v4.1-flash")).toHaveLength(1);
  expect(
    screen.queryByText("deepseek-ai/DeepSeek-V4.1-Flash"),
  ).not.toBeInTheDocument();
  expect(screen.getAllByText("glm-5.2-fp8")).toHaveLength(1);
  expect(screen.queryByText("zai-org/GLM-5.2-FP8")).not.toBeInTheDocument();
  expect(screen.getByText("deepseek-v4.1")).toBeVisible();
  expect(screen.getByText("custom/DeepSeek-V4.1-Flash")).toBeVisible();
});
