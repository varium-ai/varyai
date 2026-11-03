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
    cleanup: vi.fn(),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({
    isOpen,
    message,
    onConfirm,
    onCancel,
  }: {
    isOpen: boolean;
    message: string;
    onConfirm: () => void;
    onCancel: () => void;
  }) =>
    isOpen ? (
      <div role="dialog">
        <p>{message}</p>
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
  vi.mocked(intentRoutingClientApi.cleanup).mockResolvedValue();
});
it("distinguishes native compatibility from successful menu publication", async () => {
  vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
    enabled: true,
    recoveryPending: false,
    groupIds: [],
    nativeCompatibility: "unsupported",
  });
  mount();
  expect(
    await screen.findByText("intentRouting.nativeUnsupported"),
  ).toBeInTheDocument();
  expect(screen.getByRole("switch", { name: "Codex" })).toBeChecked();
});

it("shows restart guidance for the connected unified model menu", async () => {
  vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
    enabled: true,
    recoveryPending: false,
    groupIds: [],
  });
  mount();
  expect(
    await screen.findByText("intentRouting.clientRestart"),
  ).toBeInTheDocument();
});

it("does not show successful menu guidance while configuration recovery is pending", async () => {
  vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
    enabled: false,
    recoveryPending: true,
    groupIds: [],
  });
  mount();
  await screen.findByText("intentRouting.clientRecovery");
  expect(screen.getByRole("switch", { name: "Codex" })).toBeDisabled();
  expect(
    screen.queryByText("intentRouting.clientRestart"),
  ).not.toBeInTheDocument();
});

it("offers explicit reconnection when native authentication was revoked", async () => {
  vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
    enabled: true,
    recoveryPending: false,
    groupIds: [],
    nativeCompatibility: "reconnect_required",
  });
  const user = userEvent.setup();
  mount();
  await user.click(
    await screen.findByRole("button", {
      name: "intentRouting.nativeReconnect",
    }),
  );
  expect(intentRoutingClientApi.connect).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() =>
    expect(intentRoutingClientApi.connect).toHaveBeenCalledWith(true),
  );
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

it("confirms orphan cleanup without connecting or silently modifying configuration", async () => {
  vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
    enabled: false,
    recoveryPending: true,
    orphanedConfig: true,
    orphanCleanupAvailable: true,
    groupIds: [],
  });
  const user = userEvent.setup();
  mount();
  const cleanup = await screen.findByRole("button", {
    name: "intentRouting.clientCleanup",
  });
  expect(screen.getByRole("switch", { name: "Codex" })).not.toBeDisabled();
  expect(
    screen.queryByText("intentRouting.clientDisconnect"),
  ).not.toBeInTheDocument();
  await user.click(cleanup);
  expect(intentRoutingClientApi.cleanup).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(intentRoutingClientApi.cleanup).not.toHaveBeenCalled();
  await user.click(cleanup);
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() =>
    expect(intentRoutingClientApi.cleanup).toHaveBeenCalledOnce(),
  );
  expect(intentRoutingClientApi.connect).not.toHaveBeenCalled();
  expect(intentRoutingClientApi.disconnect).not.toHaveBeenCalled();
});

it("reconnects a restored or modified orphan directly through the Codex switch", async () => {
  vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
    enabled: false,
    recoveryPending: true,
    orphanedConfig: true,
    orphanCleanupAvailable: false,
    groupIds: [],
  });
  const user = userEvent.setup();
  mount();
  expect(
    await screen.findByText("intentRouting.clientOrphanUnsafe"),
  ).toBeInTheDocument();
  const toggle = screen.getByRole("switch", { name: "Codex" });
  expect(toggle).not.toBeDisabled();
  expect(
    screen.queryByRole("button", { name: "intentRouting.clientCleanup" }),
  ).not.toBeInTheDocument();
  await user.click(toggle);
  expect(screen.getByRole("dialog")).toHaveTextContent(
    "intentRouting.clientOrphanReconnectConsent",
  );
  expect(intentRoutingClientApi.connect).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() =>
    expect(intentRoutingClientApi.connect).toHaveBeenCalledWith(true),
  );
  expect(intentRoutingClientApi.cleanup).not.toHaveBeenCalled();
  expect(intentRoutingClientApi.disconnect).not.toHaveBeenCalled();
});

it("shows actionable recovery after a conflict and clears it after cleanup", async () => {
  vi.mocked(intentRoutingClientApi.connect).mockImplementation(async () => {
    vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
      enabled: false,
      recoveryPending: true,
      orphanedConfig: true,
      orphanCleanupAvailable: true,
      groupIds: [],
    });
    throw "intentRouting.clientConflict";
  });
  vi.mocked(intentRoutingClientApi.cleanup).mockImplementation(async () => {
    vi.mocked(intentRoutingClientApi.status).mockResolvedValue({
      enabled: false,
      recoveryPending: false,
      groupIds: [],
    });
  });
  const user = userEvent.setup();
  mount();
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Codex" })).not.toBeDisabled(),
  );
  await user.click(screen.getByRole("switch", { name: "Codex" }));
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  expect(
    await screen.findByText("intentRouting.clientOrphanRecovery"),
  ).toBeInTheDocument();
  expect(
    screen.queryByText("intentRouting.clientConflict"),
  ).not.toBeInTheDocument();
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  await user.click(
    await screen.findByRole("button", { name: "intentRouting.clientCleanup" }),
  );
  await user.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Codex" })).not.toBeDisabled(),
  );
  expect(
    screen.queryByText("intentRouting.clientConflict"),
  ).not.toBeInTheDocument();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(intentRoutingClientApi.connect).toHaveBeenCalledOnce();
});
