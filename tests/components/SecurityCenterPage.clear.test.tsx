import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SecurityCenterPage } from "@/components/security/SecurityCenterPage";

const { clearAuditLogs, getAuditLogs } = vi.hoisted(() => ({
  clearAuditLogs: vi.fn(),
  getAuditLogs: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  securityApi: {
    getRules: vi.fn().mockResolvedValue([]),
    getAuditLogs,
    clearAuditLogs,
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

async function renderPage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <SecurityCenterPage />
    </QueryClientProvider>,
  );
  await waitFor(() => expect(client.isFetching()).toBe(0));
  return screen.getByRole("button", { name: "security.clearLogs" });
}

describe("Security Center clear confirmation", () => {
  beforeEach(() => {
    clearAuditLogs.mockReset().mockResolvedValue(0);
    getAuditLogs
      .mockReset()
      .mockResolvedValue({ items: [], total: 0, page: 1, pageSize: 20 });
  });

  it("describes the deletion scope with a native title without deleting anything", async () => {
    const button = await renderPage();
    expect(button).toHaveAttribute("title", "security.clearLogsTooltip");
    expect(clearAuditLogs).not.toHaveBeenCalled();
  });

  it("opens a confirmation with the full scope and lets the user cancel", async () => {
    const button = await renderPage();
    fireEvent.change(
      screen.getByPlaceholderText("security.searchPlaceholder"),
      {
        target: { value: "synthetic-filter" },
      },
    );
    fireEvent.click(button);
    const dialog = screen.getByRole("dialog", {
      name: "security.clearLogsConfirmTitle",
    });
    expect(dialog).toHaveTextContent("security.clearLogsConfirmDescription");
    expect(clearAuditLogs).not.toHaveBeenCalled();
    const cancel = within(dialog).getByRole("button", {
      name: "common.cancel",
    });
    await waitFor(() => expect(cancel).toHaveFocus());
    fireEvent.click(cancel);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(clearAuditLogs).not.toHaveBeenCalled();
    expect(
      screen.getByPlaceholderText("security.searchPlaceholder"),
    ).toHaveValue("synthetic-filter");
  });

  it("deletes only after confirmation, prevents duplicate submissions, and reloads the records", async () => {
    const button = await renderPage();
    let finish: (count: number) => void = () => {};
    clearAuditLogs.mockImplementation(
      () =>
        new Promise<number>((resolve) => {
          finish = resolve;
        }),
    );
    fireEvent.click(button);
    const dialog = screen.getByRole("dialog");
    const confirm = within(dialog).getByRole("button", {
      name: "security.clearLogsConfirm",
    });
    fireEvent.click(confirm);
    await waitFor(() => expect(confirm).toBeDisabled());
    expect(confirm).toHaveTextContent("common.deleting");
    expect(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    fireEvent.click(confirm);
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(clearAuditLogs).toHaveBeenCalledExactlyOnceWith(undefined);
    await act(async () => finish(3));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(getAuditLogs).toHaveBeenCalledTimes(10));
  });

  it("keeps the confirmation open after failure and allows another attempt", async () => {
    const button = await renderPage();
    clearAuditLogs.mockRejectedValueOnce(new Error("Synthetic clear failure"));
    fireEvent.click(button);
    const confirm = screen.getByRole("button", {
      name: "security.clearLogsConfirm",
    });
    fireEvent.click(confirm);
    await waitFor(() => expect(clearAuditLogs).toHaveBeenCalledOnce());
    await waitFor(() => expect(confirm).toBeEnabled());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(clearAuditLogs).toHaveBeenCalledTimes(2);
  });
});
