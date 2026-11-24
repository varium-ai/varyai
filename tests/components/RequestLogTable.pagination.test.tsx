import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { toast } from "sonner";
import i18n from "i18next";
import en from "@/i18n/locales/en.json";
import { RequestLogTable } from "@/components/usage/RequestLogTable";
import { usageApi } from "@/lib/api/usage";
import type { PaginatedLogs, RequestLog } from "@/types/usage";

vi.mock("@/lib/api/usage", () => ({
  usageApi: { getRequestLogs: vi.fn() },
}));

function row(model: string): RequestLog {
  return {
    requestId: model,
    providerId: "synthetic-provider",
    providerName: "Synthetic provider",
    appType: "codex",
    model,
    costMultiplier: "1",
    inputTokens: 100,
    outputTokens: 50,
    cacheReadTokens: 20,
    cacheCreationTokens: 0,
    inputCostUsd: "0.01",
    outputCostUsd: "0.01",
    cacheReadCostUsd: "0",
    cacheCreationCostUsd: "0",
    totalCostUsd: "0.02",
    isStreaming: false,
    latencyMs: 100,
    statusCode: 200,
    createdAt: 1_700_000_000,
  };
}
function pageResult(page: number, total = 201): PaginatedLogs {
  return {
    data: Array.from(
      { length: Math.max(0, Math.min(20, total - page * 20)) },
      (_, index) => row(`synthetic-page-${page + 1}-row-${index + 1}`),
    ),
    total,
    page,
    pageSize: 20,
  };
}
function deferred() {
  let resolve!: (value: PaginatedLogs) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<PaginatedLogs>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}
function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const element = (providerName?: string) => (
    <QueryClientProvider client={client}>
      <RequestLogTable
        range={{ preset: "today" }}
        rangeLabel="Today"
        refreshIntervalMs={0}
        providerName={providerName}
      />
    </QueryClientProvider>
  );
  const view = render(element());
  return {
    ...view,
    changeProvider: (name: string) => view.rerender(element(name)),
  };
}

beforeEach(async () => {
  i18n.addResourceBundle(
    "en",
    "translation",
    { usage: en.usage, common: en.common },
    true,
    true,
  );
  await i18n.changeLanguage("en");
  vi.mocked(usageApi.getRequestLogs).mockReset();
  vi.mocked(usageApi.getRequestLogs).mockImplementation(
    async (_filters, page = 0) => pageResult(page),
  );
});

it("keeps the table and pager mounted while loading a one-row last page", async () => {
  const last = deferred();
  vi.mocked(usageApi.getRequestLogs).mockImplementation(
    async (_filters, page = 0) =>
      page === 10 ? last.promise : pageResult(page),
  );
  mount();
  await screen.findByText("synthetic-page-1-row-1");
  const table = screen.getByRole("table");
  const scrollToTable = vi.fn();
  Object.defineProperty(table.closest(".overflow-x-auto"), "scrollIntoView", {
    value: scrollToTable,
  });
  fireEvent.click(screen.getByRole("button", { name: "11" }));
  expect(screen.getByText("synthetic-page-1-row-1")).toBeVisible();
  expect(screen.getByRole("table")).toBe(table);
  expect(screen.getByRole("button", { name: "11" })).toBeDisabled();
  await act(async () => last.resolve(pageResult(10)));
  await screen.findByText("synthetic-page-11-row-1");
  expect(screen.getByRole("table")).toBe(table);
  expect(screen.getAllByRole("row")).toHaveLength(2);
  expect(scrollToTable).toHaveBeenCalledWith({ block: "start" });
  expect(screen.getByRole("button", { name: "11" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  fireEvent.click(screen.getByRole("button", { name: "10" }));
  expect(await screen.findByText("synthetic-page-10-row-1")).toBeVisible();
});

it("shows a safe retry when the last-page query fails", async () => {
  vi.mocked(usageApi.getRequestLogs).mockImplementation(
    async (_filters, page = 0) => {
      if (page === 10) throw new Error("synthetic-private-detail");
      return pageResult(page);
    },
  );
  mount();
  await screen.findByText("synthetic-page-1-row-1");
  fireEvent.click(screen.getByRole("button", { name: "11" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Query failed");
  expect(
    screen.queryByText(/synthetic-private-detail/),
  ).not.toBeInTheDocument();
  vi.mocked(usageApi.getRequestLogs).mockImplementation(
    async (_filters, page = 0) => pageResult(page),
  );
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByText("synthetic-page-11-row-1")).toBeVisible();
});

it.each([21, 0])(
  "recovers when the result count shrinks to %s while opening the last page",
  async (total) => {
    let shrunk = false;
    vi.mocked(usageApi.getRequestLogs).mockImplementation(
      async (_filters, page = 0) => {
        if (page === 10) shrunk = true;
        return pageResult(page, shrunk ? total : 201);
      },
    );
    mount();
    await screen.findByText("synthetic-page-1-row-1");
    fireEvent.click(screen.getByRole("button", { name: "11" }));
    await waitFor(() =>
      expect(usageApi.getRequestLogs).toHaveBeenLastCalledWith(
        expect.any(Object),
        Math.max(0, Math.ceil(total / 20) - 1),
        20,
      ),
    );
    if (total)
      expect(await screen.findByText("synthetic-page-2-row-1")).toBeVisible();
    else expect(await screen.findByText(en.usage.noData)).toBeVisible();
  },
);

it("does not show the previous source's rows while a different source loads", async () => {
  const other = deferred();
  vi.mocked(usageApi.getRequestLogs).mockImplementation(
    async (filters, page = 0) =>
      filters.providerName ? other.promise : pageResult(page),
  );
  const view = mount();
  await screen.findByText("synthetic-page-1-row-1");
  view.changeProvider("Other synthetic source");
  expect(screen.queryByText("synthetic-page-1-row-1")).not.toBeInTheDocument();
  await act(async () => other.resolve(pageResult(0, 0)));
  expect(await screen.findByText(en.usage.noData)).toBeVisible();
});

it("shows a warning toast when page jump input is out of range or invalid", async () => {
  const warningSpy = vi.spyOn(toast, "warning").mockImplementation(() => "");
  try {
    mount();
    await screen.findByText("synthetic-page-1-row-1");
    const callsBefore = vi.mocked(usageApi.getRequestLogs).mock.calls.length;

    const input = screen.getByPlaceholderText(en.usage.pageInputPlaceholder);
    const goButton = screen.getByRole("button", { name: en.usage.goToPage });

    fireEvent.change(input, { target: { value: "99" } });
    fireEvent.click(goButton);
    expect(warningSpy).toHaveBeenLastCalledWith(
      "Please enter a page number between 1 and 11",
    );

    fireEvent.change(input, { target: { value: "0" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(warningSpy).toHaveBeenLastCalledWith(
      "Please enter a page number between 1 and 11",
    );

    fireEvent.change(input, { target: { value: "abc" } });
    fireEvent.click(goButton);
    expect(warningSpy).toHaveBeenLastCalledWith(
      "Please enter a page number between 1 and 11",
    );
    expect(vi.mocked(usageApi.getRequestLogs).mock.calls.length).toBe(
      callsBefore,
    );

    fireEvent.change(input, { target: { value: "3" } });
    fireEvent.click(goButton);
    expect(await screen.findByText("synthetic-page-3-row-1")).toBeVisible();
    expect(input).toHaveValue("");
  } finally {
    warningSpy.mockRestore();
  }
});
