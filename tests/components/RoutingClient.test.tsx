import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, expect, it, vi } from "vitest";
import { RoutingClient } from "@/components/routing/RoutingClient";
import { intentRoutingClientApi } from "@/lib/api/intentRouting";
vi.mock("@/lib/api/intentRouting", () => ({
  intentRoutingClientApi: {
    status: vi.fn(),
    connect: vi.fn(),
    disconnect: vi.fn(),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({
    isOpen,
    onConfirm,
    onCancel,
  }: {
    isOpen: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  }) =>
    isOpen ? (
      <div role="dialog">
        <button onClick={onConfirm}>Confirm</button>
        <button onClick={onCancel}>Cancel</button>
      </div>
    ) : null,
}));
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <RoutingClient />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
    enabled: false,
    providerEnabled: false,
    recoveryPending: false,
    groupIds: [],
  });
  vi.mocked(intentRoutingClientApi.connect).mockResolvedValue({
    enabled: true,
    recoveryPending: false,
    groupIds: [],
  });
  vi.mocked(intentRoutingClientApi.disconnect).mockResolvedValue();
});
it("contains only one switch without a mode picker or publication checklist", async () => {
  mount();
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Codex" })).not.toBeDisabled(),
  );
  expect(screen.queryByRole("radiogroup")).not.toBeInTheDocument();
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  expect(
    screen.queryByText("intentRouting.clientConnect"),
  ).not.toBeInTheDocument();
});
it("confirms unified publication once on enable, without selecting groups", async () => {
  const user = userEvent.setup();
  mount();
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Codex" })).not.toBeDisabled(),
  );
  await user.click(screen.getByRole("switch", { name: "Codex" }));
  expect(intentRoutingClientApi.connect).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() =>
    expect(intentRoutingClientApi.connect).toHaveBeenCalledWith(true),
  );
});
it("canceling never changes the client configuration", async () => {
  const user = userEvent.setup();
  mount();
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Codex" })).not.toBeDisabled(),
  );
  await user.click(screen.getByRole("switch", { name: "Codex" }));
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(intentRoutingClientApi.connect).not.toHaveBeenCalled();
});
it("disconnects either old provider takeover or the unified menu with the same switch", async () => {
  vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
    enabled: false,
    providerEnabled: true,
    recoveryPending: false,
    groupIds: [],
  });
  const user = userEvent.setup();
  mount();
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Codex" })).toBeChecked(),
  );
  await user.click(screen.getByRole("switch", { name: "Codex" }));
  await waitFor(() =>
    expect(intentRoutingClientApi.disconnect).toHaveBeenCalledOnce(),
  );
});
it("offers recovery without depending on group catalogs", async () => {
  vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
    enabled: false,
    recoveryPending: true,
    groupIds: [],
  });
  const user = userEvent.setup();
  mount();
  await user.click(
    await screen.findByRole("button", {
      name: "intentRouting.clientDisconnect",
    }),
  );
  expect(intentRoutingClientApi.disconnect).toHaveBeenCalledOnce();
});
