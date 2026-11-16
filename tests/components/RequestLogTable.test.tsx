import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import {
  afterAll,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { RequestLogTable } from "@/components/usage/RequestLogTable";
import type { UsageRangeSelection } from "@/types/usage";
import i18n from "i18next";
import en from "@/i18n/locales/en.json";

const useRequestLogsMock = vi.hoisted(() => vi.fn());
const scrollIntoViewMock = vi.fn();

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: scrollIntoViewMock,
  });
});
afterAll(() => {
  Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (...args: Parameters<typeof i18n.t>) => i18n.t(...args),
    i18n: {
      resolvedLanguage: "en",
      language: "en",
    },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestLogs: (args: unknown) => useRequestLogsMock(args),
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, ...props }: any) => (
    <button {...props}>{children}</button>
  ),
}));

vi.mock("@/components/ui/input", () => ({
  Input: (props: any) => <input {...props} />,
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ children }: any) => <div>{children}</div>,
  SelectTrigger: ({ children, ...props }: any) => (
    <button type="button" {...props}>
      {children}
    </button>
  ),
  SelectValue: ({ placeholder }: any) => <span>{placeholder ?? null}</span>,
  SelectContent: () => null,
  SelectItem: () => null,
}));

vi.mock("@/components/ui/table", () => ({
  Table: ({ children }: any) => <table>{children}</table>,
  TableBody: ({ children }: any) => <tbody>{children}</tbody>,
  TableCell: ({ children, ...props }: any) => <td {...props}>{children}</td>,
  TableHead: ({ children, ...props }: any) => <th {...props}>{children}</th>,
  TableHeader: ({ children }: any) => <thead>{children}</thead>,
  TableRow: ({ children }: any) => <tr>{children}</tr>,
}));

describe("RequestLogTable", () => {
  beforeEach(() => {
    scrollIntoViewMock.mockClear();
    i18n.addResourceBundle(
      "en",
      "translation",
      { usage: en.usage },
      true,
      true,
    );
    void i18n.changeLanguage("en");
    useRequestLogsMock.mockReset();
    useRequestLogsMock.mockImplementation(
      ({ page = 0, pageSize = 20 }: { page?: number; pageSize?: number }) => ({
        data: {
          data: [],
          total: 120,
          page,
          pageSize,
        },
        isLoading: false,
      }),
    );
  });

  it("uses the plain Codex source name without changing a saved provider's name", () => {
    useRequestLogsMock.mockReturnValue({
      data: {
        data: ["_codex_session", "saved"].map((providerId) => ({
          requestId: providerId,
          providerId,
          providerName: "Codex (Session)",
          appType: "codex",
          model: "synthetic-model",
          inputTokens: 10,
          outputTokens: 5,
          cacheReadTokens: 0,
          cacheCreationTokens: 0,
          totalCostUsd: "0.5",
          latencyMs: 100,
          statusCode: 200,
          createdAt: 1000,
        })),
        total: 2,
      },
      isLoading: false,
    });
    render(
      <RequestLogTable
        range={{ preset: "today" }}
        rangeLabel="Today"
        refreshIntervalMs={0}
      />,
    );
    expect(screen.getByText("Codex")).toBeVisible();
    expect(screen.queryByText("Provider unattributed")).not.toBeInTheDocument();
    expect(screen.getByText("Codex (Session)")).toBeVisible();
    expect(
      screen.getByRole("columnheader", { name: "Provider / source" }),
    ).toBeVisible();
  });

  it("anchors the shorter last page in view instead of retaining the old bottom scroll position", () => {
    useRequestLogsMock.mockImplementation(({ page, pageSize }) => ({
      data: { data: [], total: 48, page, pageSize },
      isLoading: false,
    }));
    render(
      <RequestLogTable
        range={{ preset: "today" }}
        rangeLabel="Today"
        refreshIntervalMs={0}
      />,
    );
    expect(scrollIntoViewMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "3" }));
    expect(scrollIntoViewMock).toHaveBeenCalledWith({ block: "start" });
    expect(useRequestLogsMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ page: 2 }),
    );
  });

  it("shows the actual pricing model and retains distinct request and response identifiers", () => {
    useRequestLogsMock.mockReturnValue({
      data: {
        data: [
          {
            requestId: "synthetic",
            providerId: "supplier",
            providerName: "Synthetic",
            model: "response-model",
            pricingModel: "billing-model",
            requestModel: "configured-model",
            inputTokens: 10,
            outputTokens: 5,
            cacheReadTokens: 0,
            cacheCreationTokens: 0,
            totalCostUsd: "0.5",
            latencyMs: 100,
            statusCode: 200,
            createdAt: 1000,
          },
        ],
        total: 1,
      },
      isLoading: false,
    });
    render(
      <RequestLogTable
        range={{ preset: "today" }}
        rangeLabel="Today"
        refreshIntervalMs={0}
      />,
    );
    expect(screen.getByText("billing-model")).toBeVisible();
    expect(screen.getByText(/configured-model/)).toBeVisible();
    expect(screen.getByText(/response-model/)).toBeVisible();
  });
  it("resets pagination when the dashboard range changes", async () => {
    const initialRange: UsageRangeSelection = { preset: "today" };
    const nextRange: UsageRangeSelection = {
      preset: "custom",
      customStartDate: 1_710_000_000,
      customEndDate: 1_710_086_400,
    };

    const { rerender } = render(
      <RequestLogTable
        range={initialRange}
        rangeLabel="Today"
        appType="all"
        refreshIntervalMs={0}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "2" }));

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 1,
          range: initialRange,
        }),
      );
    });

    rerender(
      <RequestLogTable
        range={nextRange}
        rangeLabel="Custom"
        appType="all"
        refreshIntervalMs={0}
      />,
    );

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 0,
          range: nextRange,
        }),
      );
    });
  });

  it("resets pagination when the dashboard app filter changes", async () => {
    const range: UsageRangeSelection = { preset: "today" };
    const { rerender } = render(
      <RequestLogTable
        range={range}
        rangeLabel="Today"
        appType="all"
        refreshIntervalMs={0}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "2" }));

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 1,
          range,
        }),
      );
    });

    rerender(
      <RequestLogTable
        range={range}
        rangeLabel="Today"
        appType="claude"
        refreshIntervalMs={0}
      />,
    );

    await waitFor(() => {
      expect(useRequestLogsMock).toHaveBeenLastCalledWith(
        expect.objectContaining({
          page: 0,
          range,
        }),
      );
    });
  });
});
