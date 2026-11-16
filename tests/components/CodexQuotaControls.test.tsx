import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { CodexQuotaControls } from "@/components/providers/CodexQuotaControls";
import { quotaPanelApi, type QuotaControls } from "@/lib/api/quotaPanel";
vi.mock("@/lib/api/quotaPanel", () => ({
  quotaPanelApi: { codex: vi.fn(), policy: vi.fn(), reset: vi.fn() },
}));
const data: QuotaControls = {
  identity: "synthetic-identity",
  policy: { week: false, expiring: true, credits: "on" },
  resets: [{ id: "synthetic-reset", until: 2_000_000_000 }],
  pending: false,
};
function mount(
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  return render(
    <QueryClientProvider client={client}>
      <CodexQuotaControls account="synthetic" name="Synthetic account" />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(quotaPanelApi.codex).mockResolvedValue(data);
  vi.mocked(quotaPanelApi.policy).mockImplementation(
    async (_account, _identity, policy) => policy,
  );
  vi.mocked(quotaPanelApi.reset).mockResolvedValue({
    code: "reset",
    windows: 2,
  });
});
it("saves explicit automatic reset policies and mutually exclusive credit priorities", async () => {
  mount();
  fireEvent.click(
    await screen.findByRole("switch", { name: "quotaPanel.whenWeekExhausted" }),
  );
  await waitFor(() =>
    expect(quotaPanelApi.policy).toHaveBeenCalledWith(
      "synthetic",
      "synthetic-identity",
      { week: true, expiring: true, credits: "on" },
    ),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("switch", { name: "quotaPanel.creditsFirst" }),
    ).toBeEnabled(),
  );
  fireEvent.click(
    screen.getByRole("switch", { name: "quotaPanel.creditsFirst" }),
  );
  await waitFor(() =>
    expect(quotaPanelApi.policy).toHaveBeenLastCalledWith(
      "synthetic",
      "synthetic-identity",
      { week: true, expiring: true, credits: "first" },
    ),
  );
  await waitFor(() =>
    expect(
      screen.getByRole("switch", { name: "quotaPanel.creditsLast" }),
    ).toBeEnabled(),
  );
  fireEvent.click(
    screen.getByRole("switch", { name: "quotaPanel.creditsLast" }),
  );
  await waitFor(() =>
    expect(quotaPanelApi.policy).toHaveBeenLastCalledWith(
      "synthetic",
      "synthetic-identity",
      { week: true, expiring: true, credits: "last" },
    ),
  );
});
it("asks before spending a reset and targets the displayed account and reset", async () => {
  mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "quotaPanel.useReset" }),
  );
  expect(quotaPanelApi.reset).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog")).toBeVisible();
  fireEvent.click(
    screen.getByRole("button", { name: "quotaPanel.confirmReset" }),
  );
  await waitFor(() =>
    expect(quotaPanelApi.reset).toHaveBeenCalledExactlyOnceWith(
      "synthetic",
      "synthetic-identity",
      "synthetic-reset",
    ),
  );
});
it("keeps the saved policy when writing fails and never echoes backend details", async () => {
  vi.mocked(quotaPanelApi.policy).mockRejectedValue(
    new Error("private fixture detail"),
  );
  mount();
  const toggle = await screen.findByRole("switch", {
    name: "quotaPanel.whenWeekExhausted",
  });
  fireEvent.click(toggle);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "quotaPanel.failed",
  );
  expect(toggle).toHaveAttribute("aria-checked", "false");
  expect(screen.queryByText("private fixture detail")).not.toBeInTheDocument();
});
it("keeps the confirmed account identity frozen when a newer observation arrives", async () => {
  const client = new QueryClient();
  mount(client);
  fireEvent.click(
    await screen.findByRole("button", { name: "quotaPanel.useReset" }),
  );
  act(() =>
    client.setQueryData(["codex-quota-management", "synthetic"], {
      ...data,
      identity: "different-identity",
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "quotaPanel.confirmReset" }),
  );
  await waitFor(() =>
    expect(quotaPanelApi.reset).toHaveBeenCalledWith(
      "synthetic",
      "synthetic-identity",
      "synthetic-reset",
    ),
  );
});
it("retries the pending credit rather than a newly available earlier credit", async () => {
  vi.mocked(quotaPanelApi.codex).mockResolvedValue({
    ...data,
    pending: true,
    pendingCredit: "pending-credit",
    resets: [
      { id: "new-earliest", until: 2_000_000_000 },
      { id: "pending-credit", until: 2_100_000_000 },
    ],
  });
  mount();
  fireEvent.click(
    await screen.findByRole("button", { name: "quotaPanel.retryReset" }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "quotaPanel.confirmReset" }),
  );
  await waitFor(() =>
    expect(quotaPanelApi.reset).toHaveBeenCalledWith(
      "synthetic",
      "synthetic-identity",
      "pending-credit",
    ),
  );
});
